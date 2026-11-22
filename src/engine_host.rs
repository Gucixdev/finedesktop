//! Hot-reloadable engines. The layout and focus engines live in the `finedesktop-engine` crate, which is linked in
//! (the fallback) and also built as `libfinedesktop_engine.so`. When that file exists it is loaded with dlopen and every
//! call goes through its C ABI; `reload` loads a newer copy and swaps it in between two calls, so you can rebuild the
//! engines and replace them without restarting the session. Anything that goes wrong keeps the old engine running.

use std::ffi::{CStr, CString, c_void};
use std::path::{Path, PathBuf};

use finedesktop_engine::abi::{self, ArrangeIn, ArrangeOut, RectOpt, SlotC};
use finedesktop_engine::focus_engine::{self, Cardinal};
use finedesktop_engine::layout_engine::{Bar, BarKind, Layout, Params, Plan, Rect, Slot};

type ArrangeFn = unsafe extern "C" fn(*const ArrangeIn, *mut SlotC, u32, *mut ArrangeOut) -> u32;
type NeighborFn = unsafe extern "C" fn(*const RectOpt, u32, u32, u32) -> i32;

struct Loaded {
    handle: *mut c_void,
    arrange: ArrangeFn,
    neighbor: NeighborFn,
    /// private copy of the library that is actually mapped (dlopen would otherwise return the old one for the same path)
    copy: PathBuf,
}

impl Drop for Loaded {
    fn drop(&mut self) {
        unsafe { libc::dlclose(self.handle) };
        let _ = std::fs::remove_file(&self.copy);
    }
}

#[derive(Default)]
pub struct EngineHost {
    loaded: Option<Loaded>,
    generation: u32,
}

fn dl_error() -> String {
    let e = unsafe { libc::dlerror() };
    if e.is_null() { "unknown dlopen error".into() } else { unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned() }
}

fn sym<T: Copy>(h: *mut c_void, name: &str) -> Result<T, String> {
    let c = CString::new(name).unwrap();
    let p = unsafe { libc::dlsym(h, c.as_ptr()) };
    if p.is_null() { Err(format!("missing symbol {name}")) } else { Ok(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) }) }
}

impl EngineHost {
    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    /// Load `path` and replace the running engine with it. On any error the old engine stays.
    pub fn reload(&mut self, path: &Path) -> Result<String, String> {
        if !path.is_file() {
            return Err(format!("no engine library at {}", path.display()));
        }
        self.generation += 1;
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let copy = dir.join(format!("finedesktop-engine-{}-{}.so", std::process::id(), self.generation));
        std::fs::copy(path, &copy).map_err(|e| format!("copy {}: {e}", path.display()))?;
        let c = CString::new(copy.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let handle = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            let e = dl_error();
            let _ = std::fs::remove_file(&copy);
            return Err(format!("dlopen: {e}"));
        }
        let built = (|| {
            let abi_fn: unsafe extern "C" fn() -> u32 = sym(handle, "fd_engine_abi")?;
            let v = unsafe { abi_fn() };
            if v != abi::ABI {
                return Err(format!("engine ABI {v}, compositor expects {}: restart the session once", abi::ABI));
            }
            Ok((sym::<ArrangeFn>(handle, "fd_arrange")?, sym::<NeighborFn>(handle, "fd_neighbor")?))
        })();
        match built {
            Ok((arrange, neighbor)) => {
                self.loaded = Some(Loaded { handle, arrange, neighbor, copy });
                Ok(format!("engine loaded from {} (abi {})", path.display(), abi::ABI))
            }
            Err(e) => {
                unsafe { libc::dlclose(handle) };
                let _ = std::fs::remove_file(&copy);
                Err(e)
            }
        }
    }

    pub fn arrange(&self, layout: Layout, area: Rect, n: usize, focused: usize, p: &Params) -> Plan {
        let Some(l) = &self.loaded else {
            return finedesktop_engine::layout_engine::arrange(layout, area, n, focused, p);
        };
        let inp = ArrangeIn {
            layout: abi::layout_index(layout),
            area,
            n: n as u32,
            focused: focused as u32,
            nmaster: p.nmaster as u32,
            mfact: p.mfact,
            gap: p.gap,
            bar_h: p.bar_h,
        };
        let mut slots = vec![SlotC { present: 0, visible: 0, rect: area }; n];
        let mut out = ArrangeOut { has_bar: 0, bar_kind: 0, bar: area };
        let got = unsafe { (l.arrange)(&inp, slots.as_mut_ptr(), n as u32, &mut out) } as usize;
        if got != n {
            // a broken library must not leave windows unplaced: use the built-in engine for this call
            return finedesktop_engine::layout_engine::arrange(layout, area, n, focused, p);
        }
        Plan {
            slots: slots.into_iter().map(|s| (s.present != 0).then_some(Slot { rect: s.rect, visible: s.visible != 0 })).collect(),
            bar: (out.has_bar != 0).then_some(Bar { rect: out.bar, kind: if out.bar_kind == 1 { BarKind::Rows } else { BarKind::Tabs } }),
        }
    }

    pub fn neighbor(&self, rects: &[Option<Rect>], from: usize, dir: Cardinal) -> Option<usize> {
        let Some(l) = &self.loaded else { return focus_engine::neighbor(rects, from, dir) };
        let v: Vec<RectOpt> = rects
            .iter()
            .map(|r| match r {
                Some(r) => RectOpt { present: 1, rect: *r },
                None => RectOpt { present: 0, rect: Rect { x: 0, y: 0, w: 0, h: 0 } },
            })
            .collect();
        let r = unsafe { (l.neighbor)(v.as_ptr(), v.len() as u32, from as u32, abi::cardinal_index(dir)) };
        (r >= 0).then_some(r as usize)
    }

    pub fn monitor_towards(&self, geos: &[Rect], from: usize, dir: Cardinal) -> Option<usize> {
        let rects: Vec<Option<Rect>> = geos.iter().map(|g| Some(*g)).collect();
        self.neighbor(&rects, from, dir)
    }
}
