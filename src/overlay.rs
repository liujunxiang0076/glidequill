use super::*;
use glidequill::Direction;
use std::{
    io,
    sync::Mutex,
    time::{Duration, Instant},
};
use windows_sys::Win32::Graphics::GdiPlus::*;

pub const REFRESH: u32 = WM_APP + 3;
pub const TIMER: usize = 1;
const RESULT_TIME: Duration = Duration::from_millis(550);
const MAX_POINTS: usize = 512;

// The hook only appends physical coordinates. The window thread paints at most one
// frame per timer tick. No bitmap allocation or painting takes place in the hook.
#[derive(Default)]
struct Trail {
    target: usize,
    dpi: u32,
    points: Vec<POINT>,
    visible: bool,
    dirty: bool,
    result: Option<(&'static str, Instant)>,
}
static TRAIL: Mutex<Trail> = Mutex::new(Trail {
    target: 0,
    dpi: 96,
    points: Vec::new(),
    visible: false,
    dirty: false,
    result: None,
});
thread_local! {static POPUP:Cell<HWND>=const{Cell::new(null_mut())};}
fn notify(window: HWND) {
    unsafe {
        PostMessageW(window, REFRESH, 0, 0);
    }
}
pub fn begin(window: HWND, target: HWND, origin: POINT, dpi: u32) {
    let mut state = TRAIL.lock().unwrap();
    let previous = state.visible;
    state.points.clear();
    state.points.push(origin);
    state.target = target as usize;
    state.dpi = dpi;
    state.visible = false;
    state.dirty = true;
    state.result = None;
    drop(state);
    if previous {
        notify(window);
    }
}
fn append(points: &mut Vec<POINT>, pt: POINT) {
    if points.last().is_some_and(|p| p.x == pt.x && p.y == pt.y) {
        return;
    }
    if points.len() >= MAX_POINTS {
        // Keep the start and spread older samples evenly. Never truncate the live endpoint.
        for i in 1..MAX_POINTS / 2 {
            points[i] = points[2 * i];
        }
        points.truncate(MAX_POINTS / 2);
    }
    points.push(pt);
}
pub fn extend(window: HWND, pt: POINT) {
    let mut state = TRAIL.lock().unwrap();
    if state.target == 0 {
        return;
    }
    let first = !state.visible;
    append(&mut state.points, pt);
    state.visible = true;
    state.dirty = true;
    drop(state);
    if first {
        notify(window);
    }
}
pub fn finish(window: HWND, pt: POINT, direction: Option<Direction>) {
    let mut state = TRAIL.lock().unwrap();
    if state.target == 0 {
        return;
    }
    append(&mut state.points, pt);
    state.visible = true;
    state.dirty = true;
    state.result = Some((
        direction.map_or("未识别", |d| d.labels().1),
        Instant::now() + RESULT_TIME,
    ));
    drop(state);
    notify(window);
}
pub fn cancel(window: HWND) {
    let mut state = TRAIL.lock().unwrap();
    let visible = state.visible;
    *state = Trail::default();
    drop(state);
    if visible {
        notify(window);
    }
}

pub unsafe fn create() -> io::Result<()> {
    unsafe {
        let class = wide("GlideQuill.GestureTrail");
        let inst = GetModuleHandleW(null_mut());
        let wc = WNDCLASSW {
            hInstance: inst,
            lpszClassName: class.as_ptr(),
            lpfnWndProc: Some(DefWindowProcW),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(io::Error::last_os_error());
        }
        let w = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class.as_ptr(),
            null_mut(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            inst,
            null_mut(),
        );
        if w.is_null() {
            return Err(io::Error::last_os_error());
        }
        POPUP.with(|v| v.set(w));
        Result::Ok(())
    }
}
#[cfg(test)]
pub(super) fn test_popup() -> HWND {
    POPUP.with(Cell::get)
}
pub fn reset() {
    *TRAIL.lock().unwrap() = Trail::default();
    unsafe {
        KillTimer(app_window(), TIMER);
        ShowWindow(POPUP.with(Cell::get), SW_HIDE);
    }
}
pub unsafe fn destroy() {
    reset();
    unsafe {
        DestroyWindow(POPUP.with(|v| v.replace(null_mut())));
    }
}

pub unsafe fn refresh(window: HWND) {
    unsafe {
        let popup = POPUP.with(Cell::get);
        let mut state = TRAIL.lock().unwrap();
        if !state.visible
            || state.target != GetForegroundWindow() as usize
            || state.result.is_some_and(|(_, t)| Instant::now() >= t)
        {
            if state.visible {
                *state = Trail::default();
            }
            drop(state);
            KillTimer(window, TIMER);
            ShowWindow(popup, SW_HIDE);
            return;
        }
        if !state.dirty {
            return;
        }
        let (points, dpi, result) = (state.points.clone(), state.dpi, state.result.map(|r| r.0));
        state.dirty = false;
        drop(state);
        if draw(popup, &points, dpi, result) {
            ShowWindow(popup, SW_SHOWNOACTIVATE);
            if SetTimer(window, TIMER, 16, None) != 0 {
                return;
            }
        }
        reset();
    }
}

// A top-down 32-bit DIB shared by GDI+ and UpdateLayeredWindow. Only the
// bounding box of the current stroke is allocated, and only a dirty frame draws.
#[derive(Default)]
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    image: *mut GpBitmap,
    graphics: *mut GpGraphics,
}
impl Surface {
    unsafe fn new(w: i32, h: i32) -> Option<Self> {
        unsafe {
            let mut s = Self {
                dc: CreateCompatibleDC(null_mut()),
                ..Default::default()
            };
            if s.dc.is_null() {
                return None;
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = null_mut();
            s.bitmap = CreateDIBSection(s.dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if s.bitmap.is_null() || bits.is_null() {
                return None;
            }
            s.old = SelectObject(s.dc, s.bitmap);
            // GDI+ PixelFormat32bppPARGB (premultiplied BGRA, as required by AC_SRC_ALPHA).
            const PARGB: i32 = 0x000e200b;
            if GdipCreateBitmapFromScan0(w, h, w * 4, PARGB, bits.cast(), &mut s.image) != Ok
                || GdipGetImageGraphicsContext(s.image.cast(), &mut s.graphics) != Ok
            {
                return None;
            }
            GdipGraphicsClear(s.graphics, 0);
            GdipSetSmoothingMode(s.graphics, SmoothingModeAntiAlias);
            Some(s)
        }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            if !self.graphics.is_null() {
                GdipDeleteGraphics(self.graphics);
            }
            if !self.image.is_null() {
                GdipDisposeImage(self.image.cast());
            }
            if !self.old.is_null() {
                SelectObject(self.dc, self.old);
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            if !self.dc.is_null() {
                DeleteDC(self.dc);
            }
        }
    }
}
unsafe fn badge(graphics: *mut GpGraphics, label: &str, r: RECT, scale: f32) {
    unsafe {
        let mut brush = null_mut();
        let mut family = null_mut();
        let mut font = null_mut();
        let mut format = null_mut();
        if GdipCreateSolidFill(0xf5222c40, &mut brush) != Ok {
            return;
        }
        let h = r.bottom - r.top;
        let w = r.right - r.left;
        GdipFillEllipseI(graphics, brush.cast(), r.left, r.top, h, h);
        GdipFillRectangleI(graphics, brush.cast(), r.left + h / 2, r.top, w - h + 1, h);
        GdipFillEllipseI(graphics, brush.cast(), r.right - h, r.top, h, h);
        if GdipCreateFontFamilyFromName(
            wide("Microsoft YaHei UI").as_ptr(),
            null_mut(),
            &mut family,
        ) == Ok
            && GdipCreateFont(family, 14.0 * scale, FontStyleRegular, UnitPixel, &mut font) == Ok
            && GdipCreateStringFormat(StringFormatFlagsNoWrap, 0, &mut format) == Ok
        {
            GdipSetSolidFillColor(brush, 0xffffffff);
            GdipSetTextRenderingHint(graphics, TextRenderingHintAntiAliasGridFit);
            GdipSetStringFormatAlign(format, StringAlignmentCenter);
            GdipSetStringFormatLineAlign(format, StringAlignmentCenter);
            let layout = RectF {
                X: r.left as f32,
                Y: r.top as f32,
                Width: w as f32,
                Height: h as f32,
            };
            GdipDrawString(
                graphics,
                wide(label).as_ptr(),
                -1,
                font,
                &layout,
                format,
                brush.cast(),
            );
        }
        if !format.is_null() {
            GdipDeleteStringFormat(format);
        }
        if !font.is_null() {
            GdipDeleteFont(font);
        }
        if !family.is_null() {
            GdipDeleteFontFamily(family);
        }
        GdipDeleteBrush(brush.cast());
    }
}
unsafe fn draw(popup: HWND, points: &[POINT], dpi: u32, result: Option<&str>) -> bool {
    unsafe {
        let Some(&end) = points.last() else {
            return false;
        };
        let scale = dpi as f32 / 96.0;
        let px = |dip: f32| (dip * scale).ceil() as i32;
        let margin = px(5.);
        let mut area = RECT {
            left: end.x - margin,
            top: end.y - margin,
            right: end.x + margin + 1,
            bottom: end.y + margin + 1,
        };
        for p in points {
            area.left = area.left.min(p.x - margin);
            area.top = area.top.min(p.y - margin);
            area.right = area.right.max(p.x + margin + 1);
            area.bottom = area.bottom.max(p.y + margin + 1);
        }
        let mut label = RECT::default();
        if result.is_some() {
            let (w, h) = (px(100.), px(30.));
            let mut x = end.x + px(12.);
            let mut y = end.y + px(12.);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(MonitorFromPoint(end, MONITOR_DEFAULTTONEAREST), &mut info) != 0 {
                let work = info.rcMonitor;
                x = x.min(work.right - w).max(work.left);
                y = y.min(work.bottom - h).max(work.top);
            }
            label = RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            area.left = area.left.min(x);
            area.top = area.top.min(y);
            area.right = area.right.max(x + w);
            area.bottom = area.bottom.max(y + h);
        }
        let size = SIZE {
            cx: area.right - area.left,
            cy: area.bottom - area.top,
        };
        let Some(s) = Surface::new(size.cx, size.cy) else {
            return false;
        };
        let local: Vec<_> = points
            .iter()
            .map(|p| Point {
                X: p.x - area.left,
                Y: p.y - area.top,
            })
            .collect();
        for (width, color) in [(5., 0xeaffffff), (3., 0xff3868d3)] {
            let mut pen = null_mut();
            if GdipCreatePen1(color, width * scale, UnitPixel, &mut pen) == Ok {
                GdipSetPenLineCap197819(pen, LineCapRound, LineCapRound, DashCapRound);
                GdipSetPenLineJoin(pen, LineJoinRound);
                GdipDrawLinesI(s.graphics, pen, local.as_ptr(), local.len() as i32);
                GdipDeletePen(pen);
            }
        }
        if let Some(caption) = result {
            OffsetRect(&mut label, -area.left, -area.top);
            badge(s.graphics, caption, label, scale);
        }
        GdipFlush(s.graphics, FlushIntentionSync);
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let origin = POINT {
            x: area.left,
            y: area.top,
        };
        UpdateLayeredWindow(
            popup,
            null_mut(),
            &origin,
            &size,
            s.dc,
            &POINT::default(),
            0,
            &blend,
            ULW_ALPHA,
        ) != 0
    }
}
