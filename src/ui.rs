use super::*;
use windows_sys::Win32::Graphics::GdiPlus::*;
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED};

// Layout is in device-independent pixels. Convert once at each drawing/Win32 boundary.
pub const WIDTH: i32 = 812;
pub const HEIGHT: i32 = 552;
const RAIL: i32 = 168;
const LEFT: i32 = RAIL + 28;
const RIGHT: i32 = WIDTH - 28;
const FIELD: i32 = 536;
const TOP: i32 = 130;
const ROW: i32 = 54;
thread_local! { static DPI: Cell<u32> = const { Cell::new(96) }; }
pub fn set_dpi(dpi: u32) {
    DPI.with(|v| v.set(if dpi == 0 { 96 } else { dpi }));
}
pub fn px(dip: i32) -> i32 {
    DPI.with(|dpi| (dip as f64 * dpi.get() as f64 / 96.0).round() as i32)
}
pub fn dip(pixel: i32) -> i32 {
    DPI.with(|dpi| (pixel as f64 * 96.0 / dpi.get() as f64).round() as i32)
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
    ink: u32,
    grid: u32,
}
fn colors() -> Palette {
    let [
        base,
        rail,
        surface,
        text,
        muted,
        accent,
        selected,
        edge,
        ink,
        grid,
    ] = if DARK.with(Cell::get) {
        [
            0x171b22, 0x1c212a, 0x242a34, 0xeff3fa, 0xaeb8c8, 0xa3beff, 0x303e58, 0x38404e,
            0x182a4a, 0x414b5b,
        ]
    } else {
        [
            0xf6f7f9, 0xfcfcfd, 0xffffff, 0x1e2735, 0x596577, 0x325dab, 0xeaf0fc, 0xdbe0e8,
            0xffffff, 0xd7dfe9,
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
        ink,
        grid,
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
        SCOPE => [12, 100, 144, 36],
        BROWSER => [12, 140, 144, 36],
        THEME => [12, HEIGHT - 50, 144, 36],
        TOGGLE => [12, HEIGHT - 93, 144, 36],
        SAVE => [RIGHT - 94, 30, 94, 34],
        PAD => [LEFT, 402, RIGHT - LEFT, 96],
        RESULT => [LEFT, 506, RIGHT - LEFT, 28],
        _ => [
            FIELD,
            TOP + (id - FIRST) * ROW + 12,
            RIGHT - FIELD - 18,
            330,
        ],
    }
}
pub unsafe fn layout() {
    unsafe {
        for id in [
            SCOPE,
            BROWSER,
            THEME,
            TOGGLE,
            SAVE,
            PAD,
            RESULT,
            FIRST,
            FIRST + 1,
            FIRST + 2,
            FIRST + 3,
        ] {
            let h = GetDlgItem(app_window(), id);
            let [x, y, w, height] = control_bounds(id).map(px);
            SetWindowPos(
                h,
                null_mut(),
                x,
                y,
                w,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
            if (FIRST..FIRST + 4).contains(&id) {
                SendMessageW(h, CB_SETITEMHEIGHT, 0, px(29) as isize);
                SendMessageW(h, CB_SETITEMHEIGHT, usize::MAX, px(26) as isize);
            }
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
        for (id, name) in [
            (SCOPE, "通用规则"),
            (BROWSER, "浏览器"),
            (SAVE, "保存规则"),
            (TOGGLE, "启用全局手势"),
            (THEME, "深色"),
        ] {
            control(
                "BUTTON",
                name,
                id,
                WS_TABSTOP | BS_OWNERDRAW as u32,
                control_bounds(id),
            );
        }
        for id in FIRST..FIRST + 4 {
            let field = control(
                "COMBOBOX",
                "",
                id,
                WS_TABSTOP
                    | WS_VSCROLL
                    | CBS_DROPDOWNLIST as u32
                    | CBS_OWNERDRAWFIXED as u32
                    | CBS_HASSTRINGS as u32,
                control_bounds(id),
            );
            SetWindowSubclass(field, Some(action_field), 1, 0);
        }
        control(
            "GlideQuill.Practice",
            "试画区域",
            PAD,
            0,
            control_bounds(PAD),
        );
        control(
            "STATIC",
            "已暂停 · 可在上方试画",
            RESULT,
            windows_sys::Win32::System::SystemServices::SS_OWNERDRAW,
            control_bounds(RESULT),
        );
        layout();
    }
}
unsafe fn caption(w: HWND) -> String {
    let mut buf = [0; 256];
    let n = unsafe { GetWindowTextW(w, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n as usize])
}
// Keep native keyboard, accessibility and dropdown handling.
unsafe extern "system" fn action_field(
    w: HWND,
    m: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    unsafe {
        if matches!(m, WM_PAINT | WM_PRINTCLIENT | WM_PRINT) {
            let mut ps = PAINTSTRUCT::default();
            let dc = if m == WM_PAINT {
                BeginPaint(w, &mut ps)
            } else {
                wp as HDC
            };
            let mut r = RECT::default();
            GetClientRect(w, &mut r);
            let p = colors();
            let focus = windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus() == w;
            fill(dc, r, p.surface);
            panel(dc, r, p.surface, if focus { p.accent } else { p.edge }, 5);
            let chevron = RECT {
                left: r.right - px(30),
                ..r
            };
            text(dc, "\u{e70d}", chevron, 11, Face::Icon, p.muted, DT_CENTER);
            r.left += px(12);
            r.right -= px(32);
            text(dc, &caption(w), r, 14, Face::Body, p.text, 0);
            if m == WM_PAINT {
                EndPaint(w, &ps);
            }
            return 0;
        }
        let ret = DefSubclassProc(w, m, wp, lp);
        if matches!(m, WM_SETFOCUS | WM_KILLFOCUS | CB_SETCURSEL | WM_ENABLE) {
            InvalidateRect(w, null_mut(), 0);
        }
        ret
    }
}
pub unsafe fn paint(w: HWND) {
    unsafe {
        let p = colors();
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(w, &mut ps);
        fill(dc, bounds(0, 0, WIDTH, HEIGHT), p.base);
        fill(dc, bounds(0, 0, RAIL, HEIGHT), p.rail);
        fill(dc, bounds(RAIL - 1, 0, RAIL, HEIGHT), p.edge);
        DrawIconEx(
            dc,
            px(17),
            px(23),
            GetClassLongPtrW(w, GCLP_HICON) as HICON,
            px(32),
            px(32),
            0,
            null_mut(),
            DI_NORMAL,
        );
        text(
            dc,
            "GlideQuill",
            bounds(56, 20, 166, 57),
            20,
            Face::Brand,
            p.text,
            0,
        );
        text(
            dc,
            "规则范围",
            bounds(22, 67, 150, 97),
            12,
            Face::Body,
            p.muted,
            0,
        );
        fill(
            dc,
            bounds(18, HEIGHT - 106, RAIL - 18, HEIGHT - 105),
            p.edge,
        );
        let browser = GROUP.with(Cell::get);
        text(
            dc,
            if browser {
                "浏览器规则"
            } else {
                "通用手势"
            },
            bounds(LEFT, 18, 525, 57),
            24,
            Face::Strong,
            p.text,
            0,
        );
        text(
            dc,
            if browser {
                "适用于 Chrome、Edge 和 Firefox，可单独指定动作。"
            } else {
                "设置在普通窗口中划动鼠标时执行的动作。"
            },
            bounds(LEFT, 54, RIGHT - 100, 80),
            13,
            Face::Body,
            p.muted,
            0,
        );
        panel(
            dc,
            bounds(LEFT, 96, RIGHT, TOP + ROW * 4),
            p.surface,
            p.edge,
            9,
        );
        text(
            dc,
            "手势方向",
            bounds(LEFT + 18, 96, FIELD, TOP),
            12,
            Face::Body,
            p.muted,
            0,
        );
        text(
            dc,
            "执行动作",
            bounds(FIELD, 96, RIGHT, TOP),
            12,
            Face::Body,
            p.muted,
            0,
        );
        for (i, (arrow, name)) in [
            ("\u{e72b}", "向左划"),
            ("\u{e72a}", "向右划"),
            ("\u{e74a}", "向上划"),
            ("\u{e74b}", "向下划"),
        ]
        .iter()
        .enumerate()
        {
            let y = TOP + i as i32 * ROW;
            fill(dc, bounds(LEFT + 1, y, RIGHT - 1, y + 1), p.edge);
            let badge = bounds(LEFT + 18, y + 12, LEFT + 48, y + 42);
            rounded(dc, badge, p.selected, 8);
            text(dc, arrow, badge, 15, Face::Icon, p.accent, DT_CENTER);
            text(
                dc,
                name,
                bounds(LEFT + 61, y, FIELD - 12, y + ROW),
                15,
                Face::Body,
                p.text,
                0,
            );
        }
        text(
            dc,
            "手势试画",
            bounds(LEFT, 360, RIGHT, 397),
            17,
            Face::Strong,
            p.text,
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
        let mut r = item.rcItem;
        if (FIRST..FIRST + 4).contains(&id) {
            let index = item.itemID as usize;
            let name = ACTIONS
                .get(index.saturating_add(usize::from(!GROUP.with(Cell::get))))
                .map_or("", |v| v.1);
            fill(
                dc,
                r,
                if item.itemState & ODS_SELECTED != 0 {
                    p.selected
                } else {
                    p.surface
                },
            );
            r.left += px(10);
            text(dc, name, r, 14, Face::Body, p.text, 0);
        } else if id == RESULT {
            fill(dc, r, p.base);
            text(dc, &caption(item.hwndItem), r, 12, Face::Body, p.muted, 0);
        } else {
            let nav = id == SCOPE || id == BROWSER;
            let selected = nav && (id == BROWSER) == GROUP.with(Cell::get);
            let pressed = item.itemState & ODS_SELECTED != 0;
            let mut label = caption(item.hwndItem);
            fill(dc, r, if id == SAVE { p.base } else { p.rail });
            if id == SAVE {
                let disabled = item.itemState & ODS_DISABLED != 0;
                rounded(
                    dc,
                    r,
                    if disabled {
                        p.edge
                    } else if pressed {
                        p.text
                    } else {
                        p.accent
                    },
                    6,
                );
                text(
                    dc,
                    &label,
                    r,
                    14,
                    Face::Strong,
                    if disabled { p.muted } else { p.ink },
                    DT_CENTER,
                );
            } else if id == TOGGLE {
                let on = LISTENER.with(|v| v.borrow().is_some());
                let track = bounds(101, 11, 132, 27);
                rounded(dc, track, if on { p.accent } else { p.muted }, 8);
                rounded(
                    dc,
                    if on {
                        bounds(118, 13, 130, 25)
                    } else {
                        bounds(103, 13, 115, 25)
                    },
                    p.surface,
                    6,
                );
                text(
                    dc,
                    "全局手势",
                    bounds(10, 0, 96, 36),
                    14,
                    Face::Body,
                    p.text,
                    0,
                );
            } else {
                if selected || pressed {
                    rounded(dc, r, p.selected, 6);
                }
                let icon = match id {
                    SCOPE => "\u{e7f4}",
                    BROWSER => "\u{e774}",
                    _ if DARK.with(Cell::get) => "\u{e706}",
                    _ => "\u{e708}",
                };
                if id == THEME {
                    label.push_str("外观");
                }
                let c = if selected { p.accent } else { p.text };
                text(
                    dc,
                    icon,
                    bounds(10, 0, 34, 36),
                    16,
                    Face::Icon,
                    c,
                    DT_CENTER,
                );
                r.left += px(44);
                text(
                    dc,
                    &label,
                    r,
                    14,
                    if selected { Face::Strong } else { Face::Body },
                    c,
                    0,
                );
            }
        }
        if item.itemState & ODS_FOCUS != 0 {
            DrawFocusRect(dc, &inset(item.rcItem, px(3)));
        }
    }
}
pub unsafe fn paint_pad(dc: HDC, r: RECT, trail: &[POINT]) {
    unsafe {
        let p = colors();
        fill(dc, r, p.base);
        panel(dc, r, p.surface, p.edge, 8);
        // Tile the practice grid in one blit even during continuous pointer movement.
        let step = px(12);
        let tile_dc = CreateCompatibleDC(dc);
        let tile = CreateCompatibleBitmap(dc, step, step);
        if !tile_dc.is_null() && !tile.is_null() {
            let old = SelectObject(tile_dc, tile);
            fill(tile_dc, bounds(0, 0, 12, 12), p.surface);
            SetPixelV(tile_dc, step / 2, step / 2, p.grid);
            SelectObject(tile_dc, old);
            let pattern = CreatePatternBrush(tile);
            if !pattern.is_null() {
                FillRect(dc, &inset(r, px(6)), pattern);
                DeleteObject(pattern);
            }
        }
        if !tile.is_null() {
            DeleteObject(tile);
        }
        if !tile_dc.is_null() {
            DeleteDC(tile_dc);
        }
        if trail.len() >= 2 {
            let pixels: Vec<_> = trail
                .iter()
                .map(|pt| Point {
                    X: px(pt.x),
                    Y: px(pt.y),
                })
                .collect();
            let mut graphics = null_mut();
            let mut pen = null_mut();
            if GdipCreateFromHDC(dc, &mut graphics) == Ok
                && GdipCreatePen1(
                    0xff000000 | rgb(p.accent),
                    px(2) as f32,
                    UnitPixel,
                    &mut pen,
                ) == Ok
            {
                GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
                GdipDrawLinesI(graphics, pen, pixels.as_ptr(), pixels.len() as i32);
            }
            if !pen.is_null() {
                GdipDeletePen(pen);
            }
            if !graphics.is_null() {
                GdipDeleteGraphics(graphics);
            }
        } else {
            let cy = r.bottom / 2;
            let cx = r.right / 2;
            let label = RECT {
                left: cx - px(85),
                top: cy - px(15),
                right: cx + px(85),
                bottom: cy + px(15),
            };
            rounded(dc, label, p.surface, 8);
            text(
                dc,
                "按住鼠标右键，划一条直线",
                label,
                13,
                Face::Body,
                p.muted,
                DT_CENTER,
            );
        }
    }
}
