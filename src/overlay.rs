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
    idle_deadline: Option<Instant>,
    action: Option<Direction>,
    expires: Option<Instant>,
}
static TRAIL: Mutex<Trail> = Mutex::new(Trail {
    target: 0,
    dpi: 96,
    points: Vec::new(),
    visible: false,
    dirty: false,
    idle_deadline: None,
    action: None,
    expires: None,
});
#[derive(Clone, Copy, PartialEq)]
struct Subtitle {
    action: Direction,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    dpi: u32,
}
thread_local! {
    static POPUP:Cell<HWND>=const{Cell::new(null_mut())};
    static CAPTION:Cell<HWND>=const{Cell::new(null_mut())};
    static SHOWN:Cell<Option<Subtitle>>=const{Cell::new(None)};
}
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
    state.action = None;
    state.expires = None;
    state.idle_deadline = None;
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
pub fn extend(window: HWND, pt: POINT, direction: Option<Direction>) {
    let mut state = TRAIL.lock().unwrap();
    if state.target == 0 {
        return;
    }
    let now = Instant::now();
    if state.idle_deadline.is_some_and(|t| now >= t)
        || state
            .points
            .last()
            .is_some_and(|p| p.x == pt.x && p.y == pt.y)
    {
        return;
    }
    let timeout = settings::preferences().idle_ms;
    state.idle_deadline = (timeout > 0).then(|| now + Duration::from_millis(timeout.into()));
    state.action = direction;
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
    let now = Instant::now();
    let result_ms = settings::preferences().result_ms;
    if result_ms == 0 || state.idle_deadline.is_some_and(|t| now >= t) {
        *state = Trail::default();
        drop(state);
        notify(window);
        return;
    }
    state.idle_deadline = None;
    append(&mut state.points, pt);
    state.visible = true;
    state.dirty = true;
    state.action = direction;
    state.expires = Some(now + Duration::from_millis(result_ms.into()));
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
        let new_popup = || {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST,
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
            )
        };
        let w = new_popup();
        if w.is_null() {
            return Err(io::Error::last_os_error());
        }
        let caption = new_popup();
        if caption.is_null() {
            let error = io::Error::last_os_error();
            DestroyWindow(w);
            return Err(error);
        }
        CAPTION.with(|c| c.set(caption));
        POPUP.with(|v| v.set(w));
        Result::Ok(())
    }
}
#[cfg(test)]
pub(super) fn test_popup() -> HWND {
    POPUP.with(Cell::get)
}
#[cfg(test)]
pub(super) fn test_subtitle() -> HWND {
    CAPTION.with(Cell::get)
}
fn hide_subtitle() {
    if SHOWN.with(|v| v.take()).is_some() {
        unsafe {
            ShowWindow(CAPTION.with(Cell::get), SW_HIDE);
        }
    }
}
pub fn reset() {
    hide_subtitle();
    *TRAIL.lock().unwrap() = Trail::default();
    unsafe {
        KillTimer(app_window(), TIMER);
        ShowWindow(POPUP.with(Cell::get), SW_HIDE);
    }
}
pub unsafe fn destroy() {
    reset();
    unsafe {
        DestroyWindow(CAPTION.with(|v| v.replace(null_mut())));
        DestroyWindow(POPUP.with(|v| v.replace(null_mut())));
    }
}

pub unsafe fn refresh(window: HWND) {
    unsafe {
        let popup = POPUP.with(Cell::get);
        let mut state = TRAIL.lock().unwrap();
        if !state.visible
            || state.target != GetForegroundWindow() as usize
            || state.idle_deadline.is_some_and(|t| Instant::now() >= t)
            || state.expires.is_some_and(|t| Instant::now() >= t)
        {
            if state.visible {
                *state = Trail::default();
            }
            drop(state);
            KillTimer(window, TIMER);
            hide_subtitle();
            ShowWindow(popup, SW_HIDE);
            return;
        }
        if !state.dirty {
            return;
        }
        let (points, dpi, action, released) = (
            state.points.clone(),
            state.dpi,
            state.action,
            state.expires.is_some(),
        );
        state.dirty = false;
        drop(state);
        if draw(
            popup,
            &points,
            dpi,
            (released && action.is_none()).then_some("未识别"),
        ) {
            if IsWindowVisible(popup) == 0 {
                ShowWindow(popup, SW_SHOWNOACTIVATE);
            }
            if let (Some(action), Some(&end)) = (action, points.last()) {
                show_subtitle(action, end, dpi);
            } else {
                hide_subtitle();
            }
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
            GdipSetSmoothingMode(s.graphics, SmoothingModeAntiAlias8x8);
            GdipSetPixelOffsetMode(s.graphics, PixelOffsetModeHalf);
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
unsafe fn badge(graphics: *mut GpGraphics, label: &str, r: RECT, font_px: f32) {
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
            && GdipCreateFont(family, font_px, FontStyleRegular, UnitPixel, &mut font) == Ok
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
        let path = smooth_path(points, area.left, area.top, scale);
        for (width, color) in [(5., 0xeaffffff), (3., 0xff3868d3)] {
            let mut pen = null_mut();
            if GdipCreatePen1(color, width * scale, UnitPixel, &mut pen) == Ok {
                GdipSetPenLineCap197819(pen, LineCapRound, LineCapRound, DashCapRound);
                GdipSetPenLineJoin(pen, LineJoinRound);
                GdipDrawBeziers(s.graphics, pen, path.as_ptr(), path.len() as i32);
                GdipDeletePen(pen);
            }
        }
        if let Some(caption) = result {
            OffsetRect(&mut label, -area.left, -area.top);
            badge(s.graphics, caption, label, 14. * scale);
        }
        present(
            &s,
            popup,
            POINT {
                x: area.left,
                y: area.top,
            },
            size,
        )
    }
}
unsafe fn present(s: &Surface, popup: HWND, origin: POINT, size: SIZE) -> bool {
    unsafe {
        GdipFlush(s.graphics, FlushIntentionSync);
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
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
// Filter the display samples, then round each corner with a quadratic bounded by
// its adjacent segments. GDI+ consumes the equivalent cubic control points.
fn smooth_path(points: &[POINT], left: i32, top: i32, scale: f32) -> Vec<PointF> {
    let local = |p: &POINT| PointF {
        X: (p.x - left) as f32,
        Y: (p.y - top) as f32,
    };
    if points.len() < 2 {
        return Vec::new();
    }
    let mut knots = vec![local(&points[0])];
    for p in &points[1..points.len() - 1] {
        let p = local(p);
        let prev = knots.last().unwrap();
        if (p.X - prev.X).hypot(p.Y - prev.Y) >= 3. * scale {
            knots.push(p);
        }
    }
    let end = local(points.last().unwrap());
    knots.push(end);
    let mix = |a: PointF, b: PointF, t: f32| PointF {
        X: a.X + (b.X - a.X) * t,
        Y: a.Y + (b.Y - a.Y) * t,
    };
    let mut path = vec![knots[0]];
    for triplet in knots.windows(3) {
        let [a, corner, b] = [triplet[0], triplet[1], triplet[2]];
        let approach = |point: PointF| {
            let distance = (point.X - corner.X).hypot(point.Y - corner.Y);
            mix(corner, point, (6. * scale / distance.max(0.01)).min(0.5))
        };
        let enter = approach(a);
        let leave = approach(b);
        let start = *path.last().unwrap();
        path.extend([start, enter, enter]);
        path.extend([
            mix(enter, corner, 2. / 3.),
            mix(leave, corner, 2. / 3.),
            leave,
        ]);
    }
    path.extend([*path.last().unwrap(), end, end]);
    path
}

unsafe fn show_subtitle(action: Direction, end: POINT, fallback_dpi: u32) {
    unsafe {
        use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
        let monitor = MonitorFromPoint(end, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            hide_subtitle();
            return;
        }
        let mut dpi = 0;
        let mut dy = 0;
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dy) < 0 || dpi == 0 {
            dpi = fallback_dpi;
        }
        let scale = dpi as f32 / 96.;
        let px = |v: f32| (v * scale).round() as i32;
        let work = info.rcWork;
        let label = action.labels().1;
        let width = px(48. + label.chars().count() as f32 * 23.).min(work.right - work.left);
        let height = px(48.).min(work.bottom - work.top);
        let subtitle = Subtitle {
            action,
            x: work.left + (work.right - work.left - width) / 2,
            y: (work.bottom - px(36.) - height).max(work.top),
            width,
            height,
            dpi,
        };
        if SHOWN.with(Cell::get) == Some(subtitle) {
            return;
        }
        let Some(s) = Surface::new(width, height) else {
            hide_subtitle();
            return;
        };
        badge(
            s.graphics,
            label,
            RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            },
            22. * scale,
        );
        let popup = CAPTION.with(Cell::get);
        if present(
            &s,
            popup,
            POINT {
                x: subtitle.x,
                y: subtitle.y,
            },
            SIZE {
                cx: width,
                cy: height,
            },
        ) {
            ShowWindow(popup, SW_SHOWNOACTIVATE);
            SetWindowTextW(popup, wide(label).as_ptr());
            SHOWN.with(|v| v.set(Some(subtitle)));
        } else {
            hide_subtitle();
        }
    }
}
