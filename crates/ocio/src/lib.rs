//! OpenColorIO for PhotoCraft VFX: colour spaces and conversions exactly as Nuke does them.
//!
//! PhotoCraft links the same library Nuke uses (OpenColorIO, pinned to [`PINNED_VERSION`]), so
//! a conversion between two colour spaces of one config gives Nuke's numbers by construction.
//! The config is process-wide ([`current`]): OCIO's built-in ACES studio config by default
//! ([`DEFAULT_CONFIG`]), or a studio config from `$OCIO` or a file ([`set_current`]).
//!
//! Builds made with `PHOTOCRAFT_NO_OCIO=1` (and the web build) compile a stub: the API is the
//! same, [`available`] is false and every load fails with [`NO_OCIO`].
//!
//! This is PhotoCraft's only crate with `unsafe` code (the C API in `src/shim.cpp`); everything
//! it hands out is safe.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// The OpenColorIO version PhotoCraft is built and tested against (see `vcpkg.json`).
pub const PINNED_VERSION: &str = "2.4.2";

/// OCIO's built-in ACES 1.3 studio config, as shipped with OpenColorIO 2.4: the CG config's
/// ACES, display and utility spaces plus the camera log spaces (ARRI, RED, Sony, Canon,
/// Panasonic, Blackmagic, Apple), which plates from those cameras need.
pub const DEFAULT_CONFIG: &str = "ocio://studio-config-v2.2.0_aces-v1.3_ocio-v2.4";

/// Why OCIO calls fail in a build without it.
pub const NO_OCIO: &str = "This build of PhotoCraft has no OpenColorIO";

/// Whether this build has OpenColorIO.
pub fn available() -> bool {
    imp::AVAILABLE
}

/// The linked OpenColorIO version ("2.4.2"), or None without OCIO.
pub fn version() -> Option<String> {
    imp::version()
}

/// One colour space of a config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorSpace {
    pub name: String,
    /// Its menu group ("ACES", "Input/ARRI", "Utility", …); may be empty.
    pub family: String,
}

/// A loaded OCIO config (thread-safe; processors are cached per colour-space pair).
pub struct Config {
    raw: imp::RawConfig,
    uri: String,
    name: String,
    spaces: Vec<ColorSpace>,
    procs: Mutex<HashMap<(String, String), Arc<Processor>>>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config").field("uri", &self.uri).field("name", &self.name).field("spaces", &self.spaces.len()).finish()
    }
}

impl Config {
    /// A config from a `.ocio` file or an `ocio://` built-in URI.
    pub fn load(uri: &str) -> Result<Arc<Config>, String> {
        let raw = imp::RawConfig::from_uri(uri)?;
        Ok(Arc::new(Config::new(raw, uri)))
    }

    /// The config `$OCIO` names, as Nuke reads it.
    pub fn from_env() -> Result<Arc<Config>, String> {
        let raw = imp::RawConfig::from_env()?;
        let uri = std::env::var("OCIO").unwrap_or_default();
        Ok(Arc::new(Config::new(raw, &uri)))
    }

    fn new(raw: imp::RawConfig, uri: &str) -> Config {
        let (name, spaces) = raw.describe();
        Config { raw, uri: uri.to_string(), name, spaces, procs: Mutex::new(HashMap::new()) }
    }

    /// Where the config came from (file path or `ocio://` URI).
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// The config's own name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its active colour spaces, in config order.
    pub fn colorspaces(&self) -> &[ColorSpace] {
        &self.spaces
    }

    /// Whether `name` is one of its active colour spaces.
    pub fn has(&self, name: &str) -> bool {
        self.spaces.iter().any(|c| c.name == name)
    }

    /// The colour space a role names (`scene_linear`, `compositing_log`, …).
    pub fn role(&self, role: &str) -> Option<String> {
        self.raw.role(role)
    }

    /// The conversion from `src` to `dst` (colour space names, aliases or roles), cached.
    pub fn processor(&self, src: &str, dst: &str) -> Result<Arc<Processor>, String> {
        let key = (src.to_string(), dst.to_string());
        if let Some(p) = self.procs.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(p.clone());
        }
        let p = Arc::new(Processor { raw: self.raw.processor(src, dst)? });
        self.procs.lock().unwrap_or_else(|e| e.into_inner()).insert(key, p.clone());
        Ok(p)
    }
}

/// A CPU conversion between two colour spaces (float, unclamped, thread-safe).
pub struct Processor {
    raw: imp::RawProcessor,
}

impl std::fmt::Debug for Processor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Processor").field("noop", &self.is_noop()).finish()
    }
}

impl Processor {
    /// Whether it leaves every value unchanged.
    pub fn is_noop(&self) -> bool {
        self.raw.is_noop()
    }

    /// Converts straight RGBA pixels in place; alpha passes through.
    pub fn apply_rgba(&self, px: &mut [[f32; 4]]) {
        if !px.is_empty() && !self.is_noop() {
            self.raw.apply(px);
        }
    }

    /// One RGB value through the conversion.
    pub fn apply_rgb(&self, c: [f32; 3]) -> [f32; 3] {
        let mut px = [[c[0], c[1], c[2], 1.0]];
        self.apply_rgba(&mut px);
        [px[0][0], px[0][1], px[0][2]]
    }
}

static CURRENT: RwLock<Option<Arc<Config>>> = RwLock::new(None);

/// The process-wide config: the last [`set_current`], else [`DEFAULT_CONFIG`] (loaded once).
pub fn current() -> Result<Arc<Config>, String> {
    if let Some(c) = CURRENT.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return Ok(c.clone());
    }
    let mut slot = CURRENT.write().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = slot.as_ref() {
        return Ok(c.clone());
    }
    let c = Config::load(DEFAULT_CONFIG)?;
    *slot = Some(c.clone());
    Ok(c)
}

/// Makes `config` the process-wide config.
pub fn set_current(config: Arc<Config>) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(config);
}

#[cfg(not(ocio_stub))]
mod imp {
    use std::ffi::{CStr, CString, c_char, c_int, c_long};

    pub const AVAILABLE: bool = true;

    #[repr(C)]
    struct PcConfig {
        _private: [u8; 0],
    }
    #[repr(C)]
    struct PcProc {
        _private: [u8; 0],
    }

    unsafe extern "C" {
        fn pc_ocio_version() -> *const c_char;
        fn pc_ocio_config_from_uri(uri: *const c_char, err: *mut c_char, err_len: usize) -> *mut PcConfig;
        fn pc_ocio_config_from_env(err: *mut c_char, err_len: usize) -> *mut PcConfig;
        fn pc_ocio_config_free(c: *mut PcConfig);
        fn pc_ocio_config_name(c: *const PcConfig) -> *const c_char;
        fn pc_ocio_colorspace_count(c: *const PcConfig) -> c_int;
        fn pc_ocio_colorspace_name(c: *const PcConfig, i: c_int) -> *const c_char;
        fn pc_ocio_colorspace_family(c: *const PcConfig, i: c_int) -> *const c_char;
        fn pc_ocio_role(c: *const PcConfig, role: *const c_char) -> *const c_char;
        fn pc_ocio_processor(c: *const PcConfig, src: *const c_char, dst: *const c_char, err: *mut c_char, err_len: usize) -> *mut PcProc;
        fn pc_ocio_proc_is_noop(p: *const PcProc) -> c_int;
        fn pc_ocio_proc_apply_rgba(p: *const PcProc, data: *mut f32, width: c_long, height: c_long);
        fn pc_ocio_proc_free(p: *mut PcProc);
    }

    const ERR_LEN: usize = 1024;

    /// A C string the shim owns, copied out (None for null).
    fn text(p: *const c_char) -> Option<String> {
        // SAFETY: the shim returns null or a NUL-terminated string that outlives this call.
        (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    fn c_str(s: &str) -> Result<CString, String> {
        CString::new(s).map_err(|_| format!("`{s}` contains a NUL byte"))
    }

    fn error(buf: &[u8]) -> String {
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        let msg = String::from_utf8_lossy(&buf[..end]).into_owned();
        if msg.is_empty() { "OpenColorIO error".into() } else { msg }
    }

    pub fn version() -> Option<String> {
        // SAFETY: returns a static string.
        text(unsafe { pc_ocio_version() })
    }

    pub struct RawConfig(*mut PcConfig);
    // SAFETY: an OCIO config is immutable once loaded and safe to share between threads.
    unsafe impl Send for RawConfig {}
    unsafe impl Sync for RawConfig {}

    impl RawConfig {
        pub fn from_uri(uri: &str) -> Result<RawConfig, String> {
            let u = c_str(uri)?;
            let mut buf = vec![0u8; ERR_LEN];
            // SAFETY: valid C string and error buffer of ERR_LEN bytes.
            let p = unsafe { pc_ocio_config_from_uri(u.as_ptr(), buf.as_mut_ptr().cast(), ERR_LEN) };
            if p.is_null() { Err(error(&buf)) } else { Ok(RawConfig(p)) }
        }

        pub fn from_env() -> Result<RawConfig, String> {
            let mut buf = vec![0u8; ERR_LEN];
            // SAFETY: error buffer of ERR_LEN bytes.
            let p = unsafe { pc_ocio_config_from_env(buf.as_mut_ptr().cast(), ERR_LEN) };
            if p.is_null() { Err(error(&buf)) } else { Ok(RawConfig(p)) }
        }

        pub fn describe(&self) -> (String, Vec<super::ColorSpace>) {
            // SAFETY: self.0 is a live config; indices are within its count.
            unsafe {
                let name = text(pc_ocio_config_name(self.0)).unwrap_or_default();
                let n = pc_ocio_colorspace_count(self.0);
                let spaces = (0..n)
                    .filter_map(|i| {
                        let name = text(pc_ocio_colorspace_name(self.0, i))?;
                        Some(super::ColorSpace { name, family: text(pc_ocio_colorspace_family(self.0, i)).unwrap_or_default() })
                    })
                    .collect();
                (name, spaces)
            }
        }

        pub fn role(&self, role: &str) -> Option<String> {
            let r = c_str(role).ok()?;
            // SAFETY: live config and valid C string.
            text(unsafe { pc_ocio_role(self.0, r.as_ptr()) })
        }

        pub fn processor(&self, src: &str, dst: &str) -> Result<RawProcessor, String> {
            let (s, d) = (c_str(src)?, c_str(dst)?);
            let mut buf = vec![0u8; ERR_LEN];
            // SAFETY: live config, valid C strings, error buffer of ERR_LEN bytes.
            let p = unsafe { pc_ocio_processor(self.0, s.as_ptr(), d.as_ptr(), buf.as_mut_ptr().cast(), ERR_LEN) };
            if p.is_null() { Err(error(&buf)) } else { Ok(RawProcessor(p)) }
        }
    }

    impl Drop for RawConfig {
        fn drop(&mut self) {
            // SAFETY: owned pointer from the shim, freed once.
            unsafe { pc_ocio_config_free(self.0) }
        }
    }

    pub struct RawProcessor(*mut PcProc);
    // SAFETY: OCIO CPU processors are immutable and safe to apply from several threads.
    unsafe impl Send for RawProcessor {}
    unsafe impl Sync for RawProcessor {}

    impl RawProcessor {
        pub fn is_noop(&self) -> bool {
            // SAFETY: live processor.
            unsafe { pc_ocio_proc_is_noop(self.0) != 0 }
        }

        pub fn apply(&self, px: &mut [[f32; 4]]) {
            // Rows of at most 1M pixels keep `long` (32-bit on Windows) in range.
            for chunk in px.chunks_mut(1 << 20) {
                // SAFETY: `chunk` is contiguous packed RGBA f32 of `chunk.len()` pixels.
                unsafe { pc_ocio_proc_apply_rgba(self.0, chunk.as_mut_ptr().cast(), chunk.len() as c_long, 1) }
            }
        }
    }

    impl Drop for RawProcessor {
        fn drop(&mut self) {
            // SAFETY: owned pointer from the shim, freed once.
            unsafe { pc_ocio_proc_free(self.0) }
        }
    }
}

#[cfg(ocio_stub)]
mod imp {
    pub const AVAILABLE: bool = false;

    pub fn version() -> Option<String> {
        None
    }

    pub struct RawConfig;

    impl RawConfig {
        pub fn from_uri(_uri: &str) -> Result<RawConfig, String> {
            Err(super::NO_OCIO.into())
        }
        pub fn from_env() -> Result<RawConfig, String> {
            Err(super::NO_OCIO.into())
        }
        pub fn describe(&self) -> (String, Vec<super::ColorSpace>) {
            (String::new(), Vec::new())
        }
        pub fn role(&self, _role: &str) -> Option<String> {
            None
        }
        pub fn processor(&self, _src: &str, _dst: &str) -> Result<RawProcessor, String> {
            Err(super::NO_OCIO.into())
        }
    }

    pub struct RawProcessor;

    impl RawProcessor {
        pub fn is_noop(&self) -> bool {
            true
        }
        pub fn apply(&self, _px: &mut [[f32; 4]]) {}
    }
}

#[cfg(all(test, not(ocio_stub)))]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3], tol: f32) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
    }

    #[test]
    fn links_the_pinned_version_and_loads_the_default_config() {
        assert_eq!(version().as_deref(), Some(PINNED_VERSION));
        let c = current().expect("built-in ACES studio config");
        assert!(c.colorspaces().len() > 10, "{c:?}");
        for name in ["ACEScg", "ACEScct", "ACES2065-1", "sRGB Encoded Rec.709 (sRGB)", "ARRI LogC3 (EI800)", "Log3G10 REDWideGamutRGB", "S-Log3 S-Gamut3.Cine"] {
            assert!(c.has(name), "{name} missing from {:?}", c.colorspaces().iter().map(|s| &s.name).collect::<Vec<_>>());
        }
        assert_eq!(c.role("scene_linear").as_deref(), Some("ACEScg"));
        let arri = c.colorspaces().iter().find(|s| s.name == "ARRI LogC3 (EI800)").unwrap();
        assert_eq!(arri.family, "Input/ARRI");
    }

    #[test]
    fn conversions_round_trip_and_keep_values_above_one() {
        let c = current().unwrap();
        for space in ["ACEScct", "ARRI LogC3 (EI800)", "Log3G10 REDWideGamutRGB", "ACES2065-1"] {
            let to = c.processor("ACEScg", space).unwrap();
            let back = c.processor(space, "ACEScg").unwrap();
            for v in [[0.18f32, 0.18, 0.18], [4.0, 2.0, 0.5], [0.01, 0.5, 1.0]] {
                let round = back.apply_rgb(to.apply_rgb(v));
                assert!(close(round, v, 1e-4 * v.iter().fold(1.0f32, |m, x| m.max(*x))), "{space}: {v:?} → {round:?}");
            }
        }
        // ACEScct 18% grey: 0.4135884 (the ACEScct specification's mid grey).
        let cct = c.processor("ACEScg", "ACEScct").unwrap().apply_rgb([0.18; 3]);
        assert!(close(cct, [0.413_588_4; 3], 1e-5), "{cct:?}");
        // ARRI LogC3 (EI800) puts 18% grey at 0.391 (ARRI's published mid grey; neutral greys
        // stay neutral between ACEScg and ARRI Wide Gamut 3).
        let logc = c.processor("ACEScg", "ARRI LogC3 (EI800)").unwrap().apply_rgb([0.18; 3]);
        assert!(close(logc, [0.391; 3], 1e-3), "{logc:?}");
        // Superwhites are not clamped.
        let lin = c.processor("ACEScct", "ACEScg").unwrap().apply_rgb([1.2; 3]);
        assert!(lin[0] > 20.0, "{lin:?}");
        assert!(c.processor("ACEScg", "No Such Space").is_err());
    }
}
