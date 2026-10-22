//! Process-local runtime discovery before the GUI or any model worker starts.
use std::sync::OnceLock;
static REPORT: OnceLock<serde_json::Value> = OnceLock::new();

pub(crate) fn initialize() {
    let report = REPORT.get_or_init(|| {
        #[cfg(windows)]
        {
            windows::initialize()
        }
        #[cfg(not(windows))]
        {
            serde_json::json!({"status":"system-runtime","platform":std::env::consts::OS})
        }
    });
    crate::trace::write(&format!("runtime bootstrap {report}"));
}
pub(crate) fn diagnostics() -> String {
    if REPORT.get().is_none() {
        initialize();
    }
    #[cfg(windows)]
    let value = windows::probe(REPORT.get().expect("initialized"));
    #[cfg(not(windows))]
    let value = REPORT.get().expect("initialized").clone();
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

#[cfg(any(windows, test))]
mod discovery {
    use std::{
        collections::HashMap,
        ffi::OsString,
        path::{Path, PathBuf},
    };
    pub(super) struct Paths {
        pub cuda: Option<PathBuf>,
        pub cudnn: Option<PathBuf>,
        pub python: Option<PathBuf>,
        pub directories: Vec<PathBuf>,
    }
    fn push(paths: &mut Vec<PathBuf>, path: PathBuf) {
        if path.is_absolute()
            && path.is_dir()
            && !paths
                .iter()
                .any(|p| p.as_os_str().eq_ignore_ascii_case(path.as_os_str()))
        {
            paths.push(path);
        }
    }
    fn children(path: &Path) -> Vec<PathBuf> {
        let mut paths: Vec<_> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        paths.sort_by_cached_key(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let version: Vec<u32> = name
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|p| p.parse().ok())
                .collect();
            std::cmp::Reverse(version)
        });
        paths
    }
    pub(super) fn discover(executable: &Path, environment: &HashMap<String, OsString>) -> Paths {
        let environment: HashMap<_, _> = environment
            .iter()
            .map(|(key, value)| (key.to_ascii_uppercase(), value.clone()))
            .collect();
        let env = |name: &str| {
            environment
                .get(&name.to_ascii_uppercase())
                .map(PathBuf::from)
        };
        let mut cuda = Vec::new();
        let mut cudnn = Vec::new();
        let mut python = Vec::new();
        for relative in [
            "",
            "runtime",
            "runtime/cuda",
            "runtime/cudnn",
            "runtime/cuda/bin",
            "runtime/cudnn/bin",
        ] {
            let path = executable.join(relative);
            push(&mut cuda, path.clone());
            push(&mut cudnn, path);
        }
        // Explicit user selections take priority over automatic local installations.
        if let Some(path) = env("IMAGE_STUDIO_CUDA_DIR") {
            cuda.insert(0, path);
        }
        if let Some(path) = env("IMAGE_STUDIO_CUDNN_DIR") {
            cudnn.insert(0, path);
        }
        for name in ["CUDA_PATH", "CUDA_HOME"] {
            if let Some(root) = env(name) {
                push(&mut cuda, root.join("bin"));
                push(&mut cuda, root.join("bin/x64"));
            }
        }
        let mut versions: Vec<_> = environment
            .keys()
            .filter(|k| k.starts_with("CUDA_PATH_V12_"))
            .collect();
        versions.sort_by_key(|name| {
            std::cmp::Reverse(
                name.rsplit('_')
                    .next()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0),
            )
        });
        for name in versions {
            if let Some(root) = env(name) {
                push(&mut cuda, root.join("bin"));
            }
        }
        if let Some(root) = env("ProgramFiles") {
            for root in children(&root.join("NVIDIA GPU Computing Toolkit/CUDA"))
                .into_iter()
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("v12."))
                })
            {
                push(&mut cuda, root.join("bin"));
                push(&mut cuda, root.join("bin/x64"));
            }
        }
        if let Some(path) = env("IMAGE_STUDIO_PYTHON") {
            python.push(path);
        }
        for path in [
            executable.join("runtime/python/python.exe"),
            executable.join("python/python.exe"),
        ] {
            if path.is_file() {
                python.push(path);
            }
        }
        if let Some(root) = env("VIRTUAL_ENV") {
            let path = root.join("Scripts/python.exe");
            if path.is_file() {
                python.push(path);
            }
        }
        if let Some(root) = env("LOCALAPPDATA") {
            for root in children(&root.join("Programs/Python")) {
                let path = root.join("python.exe");
                if path.is_file() {
                    python.push(path);
                }
            }
        }
        for interpreter in &python {
            if let Some(root) = interpreter.parent() {
                for root in [
                    root.to_path_buf(),
                    root.parent().unwrap_or(root).to_path_buf(),
                ] {
                    push(&mut cudnn, root.join("Lib/site-packages/torch/lib"));
                    push(&mut cudnn, root.join("Lib/site-packages/nvidia/cudnn/bin"));
                }
            }
        }
        if let Some(root) = env("APPDATA") {
            for root in children(&root.join("Python")) {
                push(&mut cudnn, root.join("site-packages/torch/lib"));
                push(&mut cudnn, root.join("site-packages/nvidia/cudnn/bin"));
            }
        }
        if let Some(path) = environment.get("PATH") {
            for path in std::env::split_paths(path) {
                push(&mut cuda, path.clone());
                push(&mut cudnn, path);
            }
        }
        let selected_cuda = cuda.into_iter().find(|p| {
            p.is_absolute()
                && ["cudart64_12.dll", "curand64_10.dll", "cublas64_12.dll"]
                    .iter()
                    .all(|dll| p.join(dll).is_file())
        });
        let selected_cudnn = cudnn
            .into_iter()
            .find(|p| p.is_absolute() && p.join("cudnn64_9.dll").is_file());
        let mut directories = Vec::new();
        // CUDA is first, so bundled PyTorch CUDA DLLs do not replace the selected toolkit.
        for path in selected_cuda.iter().chain(selected_cudnn.iter()) {
            push(&mut directories, path.clone());
        }
        let python = python.into_iter().find(|p| p.is_absolute() && p.is_file());
        Paths {
            cuda: selected_cuda,
            cudnn: selected_cudnn,
            python,
            directories,
        }
    }
    pub(super) fn prepend(
        paths: &[PathBuf],
        previous: &std::ffi::OsStr,
    ) -> Result<OsString, std::env::JoinPathsError> {
        std::env::join_paths(paths.iter().cloned().chain(std::env::split_paths(previous)))
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        fn touch(root: &Path, files: &[&str]) {
            std::fs::create_dir_all(root).unwrap();
            for file in files {
                std::fs::write(root.join(file), b"fixture").unwrap();
            }
        }
        #[test]
        fn selects_complete_cuda_and_torch_cudnn_without_cwd_or_shell() {
            let root = tempfile::tempdir().unwrap();
            let app = root.path().join("Application");
            let local = root.path().join("User");
            let toolkit = root.path().join("CUDA/bin");
            touch(
                &toolkit,
                &["cudart64_12.dll", "curand64_10.dll", "cublas64_12.dll"],
            );
            let python = local.join("Programs/Python/Python311");
            touch(&python, &["python.exe"]);
            let torch = python.join("Lib/site-packages/torch/lib");
            touch(&torch, &["cudnn64_9.dll"]);
            let env = HashMap::from([
                (
                    "CUDA_PATH".into(),
                    root.path().join("CUDA").into_os_string(),
                ),
                ("LOCALAPPDATA".into(), local.into_os_string()),
            ]);
            let result = discover(&app, &env);
            assert_eq!(result.cuda, Some(toolkit.clone()));
            assert_eq!(result.cudnn, Some(torch.clone()));
            assert_eq!(result.directories, vec![toolkit, torch]);
            assert_eq!(result.python, Some(python.join("python.exe")));
        }
        #[test]
        fn windows_environment_is_case_insensitive_and_toolkit_versions_are_numeric() {
            let root = tempfile::tempdir().unwrap();
            let program = root.path().join("Program Files");
            let cuda = program.join("NVIDIA GPU Computing Toolkit/CUDA");
            for version in ["v12.9", "v12.10"] {
                touch(
                    &cuda.join(version).join("bin"),
                    &["cudart64_12.dll", "curand64_10.dll", "cublas64_12.dll"],
                );
            }
            let result = discover(
                &root.path().join("App"),
                &HashMap::from([("ProgramFiles".into(), program.into_os_string())]),
            );
            assert_eq!(result.cuda, Some(cuda.join("v12.10/bin")));
        }

        #[test]
        fn overrides_bundle_and_ignores_incomplete_or_relative_locations() {
            let root = tempfile::tempdir().unwrap();
            let app = root.path().join("App");
            touch(&app.join("runtime/cudnn"), &["cudnn64_9.dll"]);
            let explicit = root.path().join("Chosen");
            touch(&explicit, &["cudnn64_9.dll"]);
            let env = HashMap::from([
                (
                    "IMAGE_STUDIO_CUDNN_DIR".into(),
                    explicit.clone().into_os_string(),
                ),
                ("CUDA_PATH".into(), "relative".into()),
            ]);
            let result = discover(&app, &env);
            assert_eq!(result.cudnn, Some(explicit));
            assert!(result.cuda.is_none());
            let joined = prepend(&result.directories, std::ffi::OsStr::new("old-path")).unwrap();
            assert!(std::env::split_paths(&joined).any(|p| p == Path::new("old-path")));
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // Windows loader FFI is isolated here; every call documents its pointer lifetime.
mod windows {
    // CUDA's linked C++ kernels register their fatbins in .CRT$XCU, before Rust main.
    // An earlier C initializer prepares DLL discovery before those registrations.
    // It uses only process-local environment, filesystem and Windows loader APIs;
    // GUI, tracing, thread setup and model admission remain in normal Rust main.
    #[cfg(all(target_env = "msvc", feature = "image-cuda"))]
    #[used]
    #[link_section = ".CRT$XIB"]
    static BEFORE_CUDA_CONSTRUCTORS: extern "C" fn() -> i32 = initialize_before_cuda;

    #[cfg(all(target_env = "msvc", feature = "image-cuda"))]
    extern "C" fn initialize_before_cuda() -> i32 {
        super::REPORT.get_or_init(initialize);
        0
    }

    use super::discovery;
    use std::{
        collections::HashMap,
        ffi::OsStr,
        os::windows::ffi::OsStrExt,
        path::{Path, PathBuf},
    };
    use windows_sys::Win32::System::LibraryLoader::{
        AddDllDirectory, GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LoadLibraryExW,
        SetDefaultDllDirectories, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
    };
    fn wide(path: impl AsRef<OsStr>) -> Vec<u16> {
        path.as_ref().encode_wide().chain(Some(0)).collect()
    }
    fn load(path: &Path, flags: u32) -> Result<(), String> {
        let name = wide(path);
        // SAFETY: the path is NUL-terminated and the module is retained until process exit.
        if unsafe { LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), flags) }.is_null() {
            Err(format!(
                "{}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn initialize() -> serde_json::Value {
        std::env::set_var("PLAKAT_NO_SDPA", "1");
        let executable = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let environment = std::env::vars_os()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v))
            .collect::<HashMap<_, _>>();
        let paths = discovery::discover(&executable, &environment);
        if let Ok(path) = discovery::prepend(
            &paths.directories,
            &std::env::var_os("PATH").unwrap_or_default(),
        ) {
            std::env::set_var("PATH", path);
        }
        if std::env::var_os("IMAGE_STUDIO_PYTHON").is_none() {
            if let Some(path) = &paths.python {
                std::env::set_var("IMAGE_STUDIO_PYTHON", path);
            }
        }
        let mut errors = Vec::new();
        // SAFETY: process startup occurs before GUI/model workers. No CWD DLL search.
        if unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) } == 0 {
            errors.push(std::io::Error::last_os_error().to_string());
        }
        for path in &paths.directories {
            let wide = wide(path);
            // SAFETY: valid absolute directory; its cookie stays registered until process exit.
            if unsafe { AddDllDirectory(wide.as_ptr()) }.is_null() {
                errors.push(std::io::Error::last_os_error().to_string());
            }
        }
        if cfg!(feature = "image-cuda") {
            if let Some(root) = &paths.cuda {
                for dll in [
                    "cudart64_12.dll",
                    "curand64_10.dll",
                    "cublasLt64_12.dll",
                    "cublas64_12.dll",
                ] {
                    if let Err(error) = load(
                        &root.join(dll),
                        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
                    ) {
                        errors.push(error);
                    }
                }
            } else {
                errors.push("CUDA 12 runtime not found".into());
            }
            if let Err(error) = load(Path::new("nvcuda.dll"), LOAD_LIBRARY_SEARCH_SYSTEM32) {
                errors.push(error);
            }
        }
        if cfg!(feature = "image-cudnn") {
            if let Some(root) = &paths.cudnn {
                if let Err(error) = load(
                    &root.join("cudnn64_9.dll"),
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
                ) {
                    errors.push(error);
                }
            } else {
                errors.push("cuDNN 9 runtime not found".into());
            }
        }
        let report = serde_json::json!({"status":if errors.is_empty(){"ready"}else{"unavailable"},"cuda":paths.cuda,"cudnn":paths.cudnn,"python":paths.python,"errors":errors});
        report
    }
    fn module(name: &str) -> Option<(windows_sys::Win32::Foundation::HMODULE, PathBuf)> {
        let wide = wide(name);
        // SAFETY: only querying a module already loaded and retained by startup.
        let module = unsafe { GetModuleHandleW(wide.as_ptr()) };
        if module.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32768];
        // SAFETY: buffer has the declared length and the module handle is live.
        let length = unsafe {
            GetModuleFileNameW(
                module,
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).unwrap_or(0),
            )
        };
        if length == 0 {
            return None;
        }
        Some((
            module,
            PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])),
        ))
    }
    pub(super) fn probe(report: &serde_json::Value) -> serde_json::Value {
        let mut result = report.clone();
        let mut modules = serde_json::Map::new();
        for name in [
            "nvcuda.dll",
            "cudart64_12.dll",
            "curand64_10.dll",
            "cublas64_12.dll",
            "cudnn64_9.dll",
        ] {
            if let Some((_, path)) = module(name) {
                modules.insert(name.into(), serde_json::json!(path));
            }
        }
        result["modules"] = modules.into();
        if let Some((handle, _)) = module("cudnn64_9.dll") {
            // SAFETY: signature and export name are defined by the cuDNN 9 API.
            if let Some(function) =
                unsafe { GetProcAddress(handle, c"cudnnGetVersion".as_ptr().cast()) }
            {
                let version: unsafe extern "C" fn() -> usize =
                    unsafe { std::mem::transmute(function) };
                result["cudnn_version"] = serde_json::json!(unsafe { version() });
            }
        }
        result
    }
}
