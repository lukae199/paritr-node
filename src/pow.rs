#![allow(unsafe_code)]

use std::{
    env,
    ffi::{c_void, OsStr},
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
};

use libloading::Library;
use parking_lot::{Mutex, RwLock};
use thiserror::Error;

use crate::consensus::PowVerifier;

const FLAG_LARGE_PAGES: u32 = 1;
const FLAG_FULL_MEM: u32 = 4;
const FLAG_JIT: u32 = 8;

type Cache = c_void;
type Dataset = c_void;
type Vm = c_void;

type GetFlags = unsafe extern "C" fn() -> u32;
type AllocCache = unsafe extern "C" fn(u32) -> *mut Cache;
type InitCache = unsafe extern "C" fn(*mut Cache, *const c_void, usize);
type ReleaseCache = unsafe extern "C" fn(*mut Cache);
type AllocDataset = unsafe extern "C" fn(u32) -> *mut Dataset;
type DatasetItemCount = unsafe extern "C" fn() -> u64;
type InitDataset = unsafe extern "C" fn(*mut Dataset, *mut Cache, u64, u64);
type ReleaseDataset = unsafe extern "C" fn(*mut Dataset);
type CreateVm = unsafe extern "C" fn(u32, *mut Cache, *mut Dataset) -> *mut Vm;
type DestroyVm = unsafe extern "C" fn(*mut Vm);
type CalculateHash = unsafe extern "C" fn(*mut Vm, *const c_void, usize, *mut c_void);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RandomXMode {
    Light,
    Fast { initialization_threads: usize },
}

#[derive(Debug, Error)]
pub enum RandomXError {
    #[error("RandomX v1.2.3 shared library was not found; tried: {0}")]
    LibraryNotFound(String),
    #[error("RandomX library is missing API symbol {0}")]
    MissingSymbol(&'static str),
    #[error("RandomX cache allocation failed")]
    CacheAllocation,
    #[error("RandomX dataset allocation failed")]
    DatasetAllocation,
    #[error("RandomX VM allocation failed")]
    VmAllocation,
    #[error("RandomX v1 self-test failed (library may be v2 or corrupted)")]
    SelfTest,
}

struct Api {
    _library: Library,
    get_flags: GetFlags,
    alloc_cache: AllocCache,
    init_cache: InitCache,
    release_cache: ReleaseCache,
    alloc_dataset: AllocDataset,
    dataset_item_count: DatasetItemCount,
    init_dataset: InitDataset,
    release_dataset: ReleaseDataset,
    create_vm: CreateVm,
    destroy_vm: DestroyVm,
    calculate_hash: CalculateHash,
}

// Function pointers remain valid because `_library` stays owned by Api.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

impl Api {
    fn load(explicit: Option<&Path>) -> Result<Arc<Self>, RandomXError> {
        let candidates = library_candidates(explicit);
        let mut failures = Vec::new();
        for candidate in candidates {
            // Loading a native library is inherently unsafe. Every symbol below
            // is the stable RandomX v1 C ABI and is checked by the v1 hash vector.
            let library = match unsafe { Library::new(&candidate) } {
                Ok(library) => library,
                Err(error) => {
                    failures.push(format!("{} ({error})", candidate.to_string_lossy()));
                    continue;
                }
            };
            return unsafe { Self::from_library(library) }.map(Arc::new);
        }
        Err(RandomXError::LibraryNotFound(failures.join(", ")))
    }

    unsafe fn from_library(library: Library) -> Result<Self, RandomXError> {
        unsafe fn symbol<T: Copy>(
            library: &Library,
            name: &'static [u8],
            label: &'static str,
        ) -> Result<T, RandomXError> {
            // SAFETY: callers provide the exact signature from randomx.h. Api
            // retains the Library for at least as long as the copied pointer.
            unsafe { library.get::<T>(name) }
                .map(|symbol| *symbol)
                .map_err(|_| RandomXError::MissingSymbol(label))
        }

        Ok(Self {
            get_flags: unsafe { symbol(&library, b"randomx_get_flags\0", "randomx_get_flags")? },
            alloc_cache: unsafe {
                symbol(&library, b"randomx_alloc_cache\0", "randomx_alloc_cache")?
            },
            init_cache: unsafe { symbol(&library, b"randomx_init_cache\0", "randomx_init_cache")? },
            release_cache: unsafe {
                symbol(
                    &library,
                    b"randomx_release_cache\0",
                    "randomx_release_cache",
                )?
            },
            alloc_dataset: unsafe {
                symbol(
                    &library,
                    b"randomx_alloc_dataset\0",
                    "randomx_alloc_dataset",
                )?
            },
            dataset_item_count: unsafe {
                symbol(
                    &library,
                    b"randomx_dataset_item_count\0",
                    "randomx_dataset_item_count",
                )?
            },
            init_dataset: unsafe {
                symbol(&library, b"randomx_init_dataset\0", "randomx_init_dataset")?
            },
            release_dataset: unsafe {
                symbol(
                    &library,
                    b"randomx_release_dataset\0",
                    "randomx_release_dataset",
                )?
            },
            create_vm: unsafe { symbol(&library, b"randomx_create_vm\0", "randomx_create_vm")? },
            destroy_vm: unsafe { symbol(&library, b"randomx_destroy_vm\0", "randomx_destroy_vm")? },
            calculate_hash: unsafe {
                symbol(
                    &library,
                    b"randomx_calculate_hash\0",
                    "randomx_calculate_hash",
                )?
            },
            _library: library,
        })
    }
}

struct Context {
    api: Arc<Api>,
    seed: Vec<u8>,
    flags: u32,
    cache: NonNull<Cache>,
    dataset: Option<NonNull<Dataset>>,
    idle_vms: Mutex<Vec<NonNull<Vm>>>,
}

unsafe impl Send for Context {}
unsafe impl Sync for Context {}

impl Context {
    fn new(api: Arc<Api>, seed: &[u8], mode: RandomXMode) -> Result<Arc<Self>, RandomXError> {
        let mut flags = unsafe { (api.get_flags)() } & !FLAG_FULL_MEM;
        // Large pages require OS privileges and must be an explicit deployment
        // optimization, never a correctness requirement.
        flags &= !FLAG_LARGE_PAGES;
        let cache = NonNull::new(unsafe { (api.alloc_cache)(flags) })
            .ok_or(RandomXError::CacheAllocation)?;
        unsafe {
            (api.init_cache)(cache.as_ptr(), seed.as_ptr().cast(), seed.len());
        }

        let dataset = if let RandomXMode::Fast {
            initialization_threads,
        } = mode
        {
            let Some(dataset) = NonNull::new(unsafe { (api.alloc_dataset)(flags | FLAG_FULL_MEM) })
            else {
                unsafe { (api.release_cache)(cache.as_ptr()) };
                return Err(RandomXError::DatasetAllocation);
            };
            let item_count = unsafe { (api.dataset_item_count)() };
            let threads = initialization_threads.max(1);
            std::thread::scope(|scope| {
                for thread in 0..threads {
                    let start = item_count * thread as u64 / threads as u64;
                    let end = item_count * (thread + 1) as u64 / threads as u64;
                    let api = Arc::clone(&api);
                    let dataset_address = dataset.as_ptr() as usize;
                    let cache_address = cache.as_ptr() as usize;
                    scope.spawn(move || unsafe {
                        (api.init_dataset)(
                            dataset_address as *mut Dataset,
                            cache_address as *mut Cache,
                            start,
                            end - start,
                        );
                    });
                }
            });
            flags |= FLAG_FULL_MEM;
            Some(dataset)
        } else {
            None
        };

        Ok(Arc::new(Self {
            api,
            seed: seed.to_vec(),
            flags,
            cache,
            dataset,
            idle_vms: Mutex::new(Vec::new()),
        }))
    }

    fn hash(&self, input: &[u8]) -> Result<[u8; 32], RandomXError> {
        let vm = self.idle_vms.lock().pop().map_or_else(
            || {
                let mut created = unsafe {
                    (self.api.create_vm)(
                        self.flags,
                        if self.dataset.is_some() {
                            std::ptr::null_mut()
                        } else {
                            self.cache.as_ptr()
                        },
                        self.dataset.map_or(std::ptr::null_mut(), NonNull::as_ptr),
                    )
                };
                // Hardened kernels may forbid executable JIT memory even when
                // the CPU recommendation includes JIT. Interpreter fallback is
                // slower but consensus-identical and keeps validation available.
                if created.is_null() && self.flags & FLAG_JIT != 0 {
                    created = unsafe {
                        (self.api.create_vm)(
                            self.flags & !FLAG_JIT,
                            if self.dataset.is_some() {
                                std::ptr::null_mut()
                            } else {
                                self.cache.as_ptr()
                            },
                            self.dataset.map_or(std::ptr::null_mut(), NonNull::as_ptr),
                        )
                    };
                }
                NonNull::new(created).ok_or(RandomXError::VmAllocation)
            },
            Ok,
        )?;
        let mut output = [0_u8; 32];
        unsafe {
            (self.api.calculate_hash)(
                vm.as_ptr(),
                input.as_ptr().cast(),
                input.len(),
                output.as_mut_ptr().cast(),
            );
        }
        self.idle_vms.lock().push(vm);
        Ok(output)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        for vm in self.idle_vms.get_mut().drain(..) {
            unsafe { (self.api.destroy_vm)(vm.as_ptr()) };
        }
        if let Some(dataset) = self.dataset {
            unsafe { (self.api.release_dataset)(dataset.as_ptr()) };
        }
        unsafe { (self.api.release_cache)(self.cache.as_ptr()) };
    }
}

pub struct RandomX {
    api: Arc<Api>,
    mode: RandomXMode,
    context: RwLock<Arc<Context>>,
    initialization: Mutex<()>,
    validation_cache: Mutex<std::collections::HashMap<crate::crypto::Hash32, [u8; 32]>>,
}

impl RandomX {
    pub fn load(path: Option<&Path>, mode: RandomXMode) -> Result<Self, RandomXError> {
        let api = Api::load(path)?;
        let context = Context::new(Arc::clone(&api), b"test key 000", RandomXMode::Light)?;
        let expected =
            hex::decode("639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f")
                .expect("fixed vector is hex");
        if context.hash(b"This is a test")?.as_slice() != expected.as_slice() {
            return Err(RandomXError::SelfTest);
        }
        Ok(Self {
            api,
            mode,
            context: RwLock::new(context),
            initialization: Mutex::new(()),
            validation_cache: Mutex::new(std::collections::HashMap::new()),
        })
    }

    fn context_for(&self, seed: &[u8]) -> Result<Arc<Context>, RandomXError> {
        {
            let current = self.context.read();
            if current.seed == seed {
                return Ok(Arc::clone(&current));
            }
        }
        // Serialize expensive cache/dataset initialization. Concurrent workers
        // must not each allocate a separate 2 GiB dataset for the same seed.
        let _initialization = self.initialization.lock();
        {
            let current = self.context.read();
            if current.seed == seed {
                return Ok(Arc::clone(&current));
            }
        }
        let replacement = Context::new(Arc::clone(&self.api), seed, self.mode)?;
        let mut current = self.context.write();
        if current.seed != seed {
            *current = replacement;
        }
        Ok(Arc::clone(&current))
    }

    pub fn calculate(&self, seed: &[u8], input: &[u8]) -> Result<[u8; 32], RandomXError> {
        self.context_for(seed)?.hash(input)
    }
}

impl PowVerifier for RandomX {
    fn hash(&self, seed: &[u8], input: &[u8]) -> Result<[u8; 32], String> {
        // A share prefix is checked repeatedly as new shares arrive. Cache only
        // actual hash results; targets and all other consensus rules still run.
        // domain_hash length-prefixes both inputs, including the epoch seed.
        let key = crate::crypto::domain_hash(seed, input);
        if let Some(hash) = self.validation_cache.lock().get(&key).copied() {
            return Ok(hash);
        }
        let hash = self
            .calculate(seed, input)
            .map_err(|error| error.to_string())?;
        let mut cache = self.validation_cache.lock();
        if cache.len() >= 8_192 {
            cache.clear();
        }
        cache.insert(key, hash);
        Ok(hash)
    }
}

fn library_candidates(explicit: Option<&Path>) -> Vec<PathBuf> {
    let filenames: &[&str] = if cfg!(target_os = "windows") {
        // MSVC builds normally omit the Unix-style `lib` prefix, while
        // MinGW release bundles commonly retain it. Accept both spellings.
        &["randomx.dll", "librandomx.dll"]
    } else if cfg!(target_os = "macos") {
        &["librandomx.dylib"]
    } else {
        &["librandomx.so"]
    };
    let mut candidates = Vec::new();
    if let Some(path) = explicit {
        candidates.push(path.to_path_buf());
    }
    if let Some(path) = env::var_os("PARITR_RANDOMX_LIBRARY") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(executable) = env::current_exe() {
        if let Some(directory) = executable.parent() {
            for filename in filenames {
                candidates.push(directory.join(filename));
                candidates.push(directory.join("lib").join(filename));
            }
        }
    }
    candidates.extend(filenames.iter().map(PathBuf::from));
    candidates.push(PathBuf::from(OsStr::new("randomx")));
    candidates
}
