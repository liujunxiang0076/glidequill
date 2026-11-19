use super::*;
use windows_sys::Win32::Graphics::GdiPlus::*;
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED};

// Layout is in device-independent pixels. Convert once at each drawing/Win32 boundary.
pub const WIDTH: i32 = 592;
pub const HEIGHT: i32 = 414;
const LEFT: i32 = 24;
const RIGHT: i32 = WIDTH - LEFT;
const ROW_START: i32 = 168;
const ROW_HEIGHT: i32 = 46;
pub const RULE_ROW: i32 = 140;
thread_local! { static DPI: Cell<u32> = const { Cell::new(96) }; }
pub fn set_dpi(dpi: u32) {
    DPI.with(|v| v.set(if dpi == 0 { 96 } else { dpi }));
}
pub fn px(dip: i32) -> i32 {
    DPI.with(|dpi| (dip as f64 * dpi.get() as f64 / 96.0).round() as i32)
}
fn bounds(x: i32, y: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left: px(x),
        top: px(y),
        right: px(right),
        bottom: px(bottom),
    }
}
fn inset(mut r: RECT, amount: i32) -> RECT {
    r.left += amount;
    r.top += amount;
    r.right -= amount;
    r.bottom -= amount;
    r
}
fn rgb(v: u32) -> u32 {
    (v >> 16) | (v & 0xff00) | ((v & 255) << 16)
}

struct Palette {
    base: u32,
    rail: u32,
    surface: u32,
    text: u32,
    muted: u32,
    accent: u32,
    selected: u32,
    edge: u32,
}
fn colors() -> Palette {
    let [base, rail, surface, text, muted, accent, selected, edge] = if DARK.with(Cell::get) {
        [
            0x171b22, 0x1c212a, 0x242a34, 0xeff3fa, 0xaeb8c8, 0xa3beff, 0x303e58, 0x38404e,
        ]
    } else {
        [
            0xf6f7f9, 0xfcfcfd, 0xffffff, 0x1e2735, 0x596577, 0x325dab, 0xeaf0fc, 0xdbe0e8,
        ]
    }
    .map(rgb);
    Palette {
        base,
        rail,
        surface,
        text,
        muted,
        accent,
        selected,
        edge,
    }
}

// Keep GDI+ active for the lifetime of the native window and its child controls.
pub struct GraphicsSession(usize);
impl GraphicsSession {
    pub fn start() -> Self {
        let mut token = 0;
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            ..Default::default()
        };
        unsafe {
            GdiplusStartup(&mut token, &input, null_mut());
        }
        Self(token)
    }
}
impl Drop for GraphicsSession {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { GdiplusShutdown(self.0) };
        }
    }
}
unsafe fn fill(dc: HDC, r: RECT, color: u32) {
    unsafe {
        SetDCBrushColor(dc, color);
        FillRect(dc, &r, GetStockObject(DC_BRUSH));
    }
}
unsafe fn rounded(dc: HDC, r: RECT, color: u32, radius: i32) {
    unsafe {
        let mut graphics = null_mut();
        let mut path = null_mut();
        let mut brush = null_mut();
        if GdipCreateFromHDC(dc, &mut graphics) == Ok
            && GdipCreatePath(FillModeAlternate, &mut path) == Ok
            && GdipCreateSolidFill(0xff000000 | rgb(color), &mut brush) == Ok
        {
            let d = (px(radius) * 2).min(r.right - r.left).min(r.bottom - r.top) as f32;
            let (x, y, right, bottom) =
                (r.left as f32, r.top as f32, r.right as f32, r.bottom as f32);
            for (ax, ay, angle) in [
                (x, y, 180.),
                (right - d, y, 270.),
                (right - d, bottom - d, 0.),
                (x, bottom - d, 90.),
            ] {
                GdipAddPathArc(path, ax, ay, d, d, angle, 90.);
            }
            GdipClosePathFigure(path);
            GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
            GdipFillPath(graphics, brush.cast(), path);
        } else {
            fill(dc, r, color);
        }
        if !brush.is_null() {
            GdipDeleteBrush(brush.cast());
        }
        if !path.is_null() {
            GdipDeletePath(path);
        }
        if !graphics.is_null() {
            GdipDeleteGraphics(graphics);
        }
    }
}
unsafe fn panel(dc: HDC, r: RECT, background: u32, edge: u32, radius: i32) {
    unsafe {
        rounded(dc, r, edge, radius);
        rounded(dc, inset(r, 1), background, radius);
    }
}
enum Face {
    Body,
    Strong,
    Brand,
    Icon,
}
unsafe fn text(dc: HDC, s: &str, mut area: RECT, size: i32, face: Face, color: u32, align: u32) {
    unsafe {
        let family = match face {
            Face::Icon => "Segoe MDL2 Assets",
            Face::Brand => "Segoe UI",
            _ => "Microsoft YaHei UI",
        };
        let weight = if matches!(face, Face::Strong | Face::Brand) {
            600
        } else {
            400
        };
        let font = CreateFontW(
            -px(size),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            wide(family).as_ptr(),
        );
        let old = SelectObject(dc, font);
        let mut graphics = null_mut();
        let mut glyph_font = null_mut();
        let mut brush = null_mut();
        let mut format = null_mut();
        let label = wide(s);
        let layout = RectF {
            X: area.left as f32,
            Y: area.top as f32,
            Width: (area.right - area.left) as f32,
            Height: (area.bottom - area.top) as f32,
        };
        let drawn = GdipCreateFromHDC(dc, &mut graphics) == Ok
            && GdipCreateFontFromDC(dc, &mut glyph_font) == Ok
            && GdipCreateSolidFill(0xff000000 | rgb(color), &mut brush) == Ok
            && GdipCreateStringFormat(StringFormatFlagsNoWrap, 0, &mut format) == Ok
            && {
                GdipSetTextRenderingHint(graphics, TextRenderingHintAntiAliasGridFit);
                GdipSetStringFormatAlign(
                    format,
                    if align == DT_CENTER {
                        StringAlignmentCenter
                    } else {
                        StringAlignmentNear
                    },
                );
                GdipSetStringFormatLineAlign(format, StringAlignmentCenter);
                GdipSetStringFormatTrimming(format, StringTrimmingEllipsisCharacter);
                GdipDrawString(
                    graphics,
                    label.as_ptr(),
                    -1,
                    glyph_font,
                    &layout,
                    format,
                    brush.cast(),
                ) == Ok
            };
        if !format.is_null() {
            GdipDeleteStringFormat(format);
        }
        if !brush.is_null() {
            GdipDeleteBrush(brush.cast());
        }
        if !glyph_font.is_null() {
            GdipDeleteFont(glyph_font);
        }
        if !graphics.is_null() {
            GdipDeleteGraphics(graphics);
        }
        if !drawn {
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, color);
            DrawTextW(
                dc,
                wide(s).as_mut_ptr(),
                -1,
                &mut area,
                DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX | align,
            );
        }
        SelectObject(dc, old);
        DeleteObject(font);
    }
}
fn control_bounds(id: i32) -> [i32; 4] {
    match id {
        THEME => [RIGHT - 89, 12, 89, 30],
        TOGGLE => [RIGHT - 113, 67, 113, 34],
        RESULT => [LEFT, 366, RIGHT - LEFT, 30],
        _ => [
            LEFT + 1,
            ROW_START + (id - RULE_ROW) * ROW_HEIGHT,
            RIGHT - LEFT - 2,
            ROW_HEIGHT,
        ],
    }
}
fn controls() -> impl Iterator<Item = i32> {
    [THEME, TOGGLE, RESULT]
        .into_iter()
        .chain(RULE_ROW..RULE_ROW + 4)
}
pub unsafe fn layout() {
    unsafe {
        for id in controls() {
            let [x, y, w, h] = control_bounds(id).map(px);
            SetWindowPos(
                GetDlgItem(app_window(), id),
                null_mut(),
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
        RedrawWindow(
            app_window(),
            null_mut(),
            null_mut(),
            RDW_INVALIDATE | RDW_ALLCHILDREN,
        );
    }
}
pub unsafe fn build() {
    unsafe {
        for (id, name) in [(THEME, "深色"), (TOGGLE, "启用全局手势")] {
            control(
                "BUTTON",
                name,
                id,
                WS_TABSTOP | BS_OWNERDRAW as u32,
                control_bounds(id),
            );
        }
        let static_style = windows_sys::Win32::System::SystemServices::SS_OWNERDRAW;
        control(
            "STATIC",
            "已暂停 · 开启全局手势即可显示划动轨迹",
            RESULT,
            static_style,
            control_bounds(RESULT),
        );
        for (index, dir) in DIRECTIONS.iter().enumerate() {
            let (name, action) = dir.labels();
            let id = RULE_ROW + index as i32;
            control(
                "STATIC",
                &format!("{name} · {action}"),
                id,
                static_style,
                control_bounds(id),
            );
        }
    }
}
unsafe fn caption(w: HWND) -> String {
    let mut buf = [0; 256];
    let n = unsafe { GetWindowTextW(w, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n as usize])
}
pub unsafe fn paint(w: HWND) {
    unsafe {
        let p = colors();
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(w, &mut ps);
        fill(dc, bounds(0, 0, WIDTH, HEIGHT), p.base);
        fill(dc, bounds(0, 0, WIDTH, 54), p.rail);
        fill(dc, bounds(0, 53, WIDTH, 54), p.edge);
        DrawIconEx(
            dc,
            px(LEFT),
            px(13),
            GetClassLongPtrW(w, GCLP_HICON) as HICON,
            px(28),
            px(28),
            0,
            null_mut(),
            DI_NORMAL,
        );
        text(
            dc,
            "GlideQuill",
            bounds(LEFT + 36, 9, 240, 44),
            19,
            Face::Brand,
            p.text,
            0,
        );
        text(
            dc,
            "手势规则",
            bounds(LEFT, 63, 300, 103),
            22,
            Face::Strong,
            p.text,
            0,
        );
        text(
            dc,
            "按住右键划动查看轨迹，松开查看识别结果。",
            bounds(LEFT, 103, RIGHT, 129),
            13,
            Face::Body,
            p.muted,
            0,
        );
        panel(
            dc,
            bounds(LEFT, 138, RIGHT, ROW_START + ROW_HEIGHT * 4 + 7),
            p.surface,
            p.edge,
            8,
        );
        text(
            dc,
            "划动方向",
            bounds(LEFT + 17, 138, 285, ROW_START),
            12,
            Face::Body,
            p.muted,
            0,
        );
        text(
            dc,
            "对应动作（仅预览）",
            bounds(304, 138, RIGHT - 12, ROW_START),
            12,
            Face::Body,
            p.muted,
            0,
        );
        EndPaint(w, &ps);
    }
}
pub unsafe fn item(item: &DRAWITEMSTRUCT) {
    unsafe {
        let p = colors();
        let id = item.CtlID as i32;
        let dc = item.hDC;
        let r = item.rcItem;
        if (RULE_ROW..RULE_ROW + 4).contains(&id) {
            let index = (id - RULE_ROW) as usize;
            let (name, action) = DIRECTIONS[index].labels();
            fill(dc, r, p.surface);
            fill(
                dc,
                RECT {
                    bottom: r.top + 1,
                    ..r
                },
                p.edge,
            );
            let badge = bounds(16, 9, 44, 37);
            rounded(dc, badge, p.selected, 7);
            text(
                dc,
                ["\u{e72b}", "\u{e72a}", "\u{e74a}", "\u{e74b}"][index],
                badge,
                14,
                Face::Icon,
                p.accent,
                DT_CENTER,
            );
            text(
                dc,
                name,
                bounds(55, 0, 258, ROW_HEIGHT),
                14,
                Face::Body,
                p.text,
                0,
            );
            text(
                dc,
                action,
                bounds(304 - LEFT - 1, 0, RIGHT - LEFT - 10, ROW_HEIGHT),
                14,
                Face::Body,
                p.text,
                0,
            );
        } else if id == RESULT {
            fill(dc, r, p.base);
            text(dc, &caption(item.hwndItem), r, 12, Face::Body, p.muted, 0);
        } else if id == TOGGLE {
            fill(dc, r, p.base);
            let on = LISTENER.with(|v| v.borrow().is_some());
            rounded(
                dc,
                bounds(79, 9, 111, 26),
                if on { p.accent } else { p.muted },
                9,
            );
            rounded(
                dc,
                if on {
                    bounds(96, 11, 109, 24)
                } else {
                    bounds(81, 11, 94, 24)
                },
                p.surface,
                7,
            );
            text(
                dc,
                "全局手势",
                bounds(4, 0, 76, 34),
                14,
                Face::Body,
                if item.itemState & ODS_DISABLED == 0 {
                    p.text
                } else {
                    p.muted
                },
                0,
            );
        } else if id == THEME {
            fill(dc, r, p.rail);
            if item.itemState & ODS_SELECTED != 0 {
                rounded(dc, r, p.selected, 6);
            }
            let icon = if DARK.with(Cell::get) {
                "\u{e706}"
            } else {
                "\u{e708}"
            };
            text(
                dc,
                icon,
                bounds(4, 0, 24, 30),
                14,
                Face::Icon,
                p.muted,
                DT_CENTER,
            );
            text(
                dc,
                &format!("{}外观", caption(item.hwndItem)),
                bounds(27, 0, 89, 30),
                13,
                Face::Body,
                p.text,
                0,
            );
        }
        if item.itemState & ODS_FOCUS != 0 {
            DrawFocusRect(dc, &inset(r, px(2)));
        }
    }
}
