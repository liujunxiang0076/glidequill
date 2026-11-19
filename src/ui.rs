use super::*;
use windows_sys::Win32::Graphics::GdiPlus::*;
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED};

// Layout is in device-independent pixels. Convert once at each drawing/Win32 boundary.
pub const WIDTH: i32 = 720;
pub const HEIGHT: i32 = 380;
const RAIL: i32 = 160;
const LEFT: i32 = RAIL + 24;
const RIGHT: i32 = WIDTH - 24;
const CARD_TOP: i32 = 94;
const SETTING_HEIGHT: i32 = 68;
const FIELD: i32 = LEFT + 260;
const ROW_START: i32 = CARD_TOP + 34;
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
        id if id == Page::General as i32 => [10, 75, RAIL - 20, 38],
        id if id == Page::Rules as i32 => [10, 118, RAIL - 20, 38],
        id if id == Page::Feedback as i32 => [10, 161, RAIL - 20, 38],
        TOGGLE if PAGE.with(Cell::get) == Page::General => [RIGHT - 125, CARD_TOP + 17, 113, 34],
        TOGGLE => [RIGHT - 113, 20, 113, 34],
        AUTOSTART => [RIGHT - 125, CARD_TOP + SETTING_HEIGHT + 17, 113, 34],
        THEME => [RIGHT - 145, CARD_TOP + 2 * SETTING_HEIGHT + 17, 132, 34],
        IDLE => [RIGHT - 145, CARD_TOP + 17, 132, 34],
        LINGER => [RIGHT - 145, CARD_TOP + SETTING_HEIGHT + 17, 132, 34],
        RESULT => [LEFT, HEIGHT - 35, RIGHT - LEFT, 26],
        _ => [
            LEFT + 1,
            ROW_START + (id - RULE_ROW) * ROW_HEIGHT,
            RIGHT - LEFT - 2,
            ROW_HEIGHT,
        ],
    }
}
fn controls() -> impl Iterator<Item = i32> {
    [THEME, TOGGLE, AUTOSTART, IDLE, LINGER, RESULT]
        .into_iter()
        .chain(Page::ALL.map(|p| p as i32))
        .chain(RULE_ROW..RULE_ROW + 4)
}
pub unsafe fn layout() {
    unsafe {
        for id in controls() {
            let page = PAGE.with(Cell::get);
            let visible = match id {
                THEME | AUTOSTART => page == Page::General,
                IDLE | LINGER => page == Page::Feedback,
                TOGGLE => page != Page::Feedback,
                RULE_ROW..=143 => page == Page::Rules,
                _ => true,
            };
            let [x, y, w, h] = control_bounds(id).map(px);
            SetWindowPos(
                GetDlgItem(app_window(), id),
                null_mut(),
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE
                    | SWP_NOZORDER
                    | if visible {
                        SWP_SHOWWINDOW
                    } else {
                        SWP_HIDEWINDOW
                    },
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
        for (id, name) in [
            (THEME, "深色"),
            (Page::General as i32, Page::General.title()),
            (Page::Rules as i32, Page::Rules.title()),
            (Page::Feedback as i32, Page::Feedback.title()),
            (TOGGLE, "启用全局手势"),
            (AUTOSTART, "启用登录自启"),
            (IDLE, "停笔取消时间"),
            (LINGER, "松键后保留时间"),
        ] {
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
        fill(dc, bounds(0, 0, RAIL, HEIGHT), p.rail);
        fill(dc, bounds(RAIL - 1, 0, RAIL, HEIGHT), p.edge);
        DrawIconEx(
            dc,
            px(17),
            px(20),
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
            bounds(50, 14, RAIL - 3, 54),
            19,
            Face::Brand,
            p.text,
            0,
        );
        let page = PAGE.with(Cell::get);
        text(
            dc,
            page.title(),
            bounds(LEFT, 14, RIGHT - 120, 55),
            23,
            Face::Strong,
            p.text,
            0,
        );
        let description = match page {
            Page::Rules => "按住右键划动，匹配的动作将在屏幕下方显示。",
            Page::General => "管理手势启停、登录启动与窗口外观。",
            Page::Feedback => "调整停笔和松键后，轨迹与动作字幕的显示时间。",
        };
        text(
            dc,
            description,
            bounds(LEFT, 54, RIGHT, 80),
            13,
            Face::Body,
            p.muted,
            0,
        );
        if page == Page::Rules {
            panel(
                dc,
                bounds(LEFT, CARD_TOP, RIGHT, ROW_START + ROW_HEIGHT * 4 + 6),
                p.surface,
                p.edge,
                8,
            );
            text(
                dc,
                "划动方向",
                bounds(LEFT + 17, CARD_TOP, FIELD, ROW_START),
                12,
                Face::Body,
                p.muted,
                0,
            );
            text(
                dc,
                "对应动作（仅预览）",
                bounds(FIELD, CARD_TOP, RIGHT, ROW_START),
                12,
                Face::Body,
                p.muted,
                0,
            );
        } else {
            let rows: &[(&str, &str)] = match page {
                Page::General => &[
                    ("全局手势", "下次启动时恢复当前状态"),
                    ("登录时自启", "登录 Windows 后在系统托盘运行"),
                    ("窗口外观", "切换浅色与深色主题"),
                ],
                _ => &[
                    ("停笔后取消手势", "按住右键停止移动后，清除本次轨迹"),
                    ("松键后保留时间", "轨迹和屏幕下方字幕同时消失"),
                ],
            };
            panel(
                dc,
                bounds(
                    LEFT,
                    CARD_TOP,
                    RIGHT,
                    CARD_TOP + SETTING_HEIGHT * rows.len() as i32,
                ),
                p.surface,
                p.edge,
                8,
            );
            for (i, (name, desc)) in rows.iter().enumerate() {
                let y = CARD_TOP + i as i32 * SETTING_HEIGHT;
                if i > 0 {
                    fill(dc, bounds(LEFT + 16, y, RIGHT - 16, y + 1), p.edge);
                }
                text(
                    dc,
                    name,
                    bounds(LEFT + 16, y + 8, RIGHT - 154, y + 35),
                    15,
                    Face::Strong,
                    p.text,
                    0,
                );
                text(
                    dc,
                    desc,
                    bounds(LEFT + 16, y + 34, RIGHT - 154, y + 59),
                    12,
                    Face::Body,
                    p.muted,
                    0,
                );
            }
        }
        fill(dc, bounds(LEFT, HEIGHT - 41, RIGHT, HEIGHT - 40), p.edge);
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
                bounds(FIELD - LEFT - 1, 0, RIGHT - LEFT - 10, ROW_HEIGHT),
                14,
                Face::Body,
                p.text,
                0,
            );
        } else if id == RESULT {
            fill(dc, r, p.base);
            text(dc, &caption(item.hwndItem), r, 12, Face::Body, p.muted, 0);
        } else if id == TOGGLE || id == AUTOSTART {
            let settings = PAGE.with(Cell::get) == Page::General;
            fill(dc, r, if settings { p.surface } else { p.base });
            let on = if id == AUTOSTART {
                STARTUP_ON.with(Cell::get)
            } else {
                LISTENER.with(|v| v.borrow().is_some())
            };
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
                if !settings {
                    "全局手势"
                } else if on {
                    "已开启"
                } else {
                    "已关闭"
                },
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
        } else if id == IDLE || id == LINGER {
            fill(dc, r, p.surface);
            panel(dc, r, p.base, p.edge, 6);
            let prefs = settings::preferences();
            let ms = if id == IDLE {
                prefs.idle_ms
            } else {
                prefs.result_ms
            };
            text(
                dc,
                time_label(id, ms),
                bounds(10, 0, 108, 34),
                14,
                Face::Body,
                p.text,
                0,
            );
            text(
                dc,
                "\u{e70d}",
                bounds(109, 0, 129, 34),
                11,
                Face::Icon,
                p.muted,
                DT_CENTER,
            );
        } else if let Some(page) = Page::ALL.into_iter().find(|&page| page as i32 == id) {
            let selected = PAGE.with(Cell::get) == page;
            fill(dc, r, p.rail);
            if selected || item.itemState & ODS_SELECTED != 0 {
                rounded(dc, r, p.selected, 7);
            }
            let color = if selected { p.accent } else { p.text };
            let icon = match page {
                Page::General => "\u{e713}",
                Page::Rules => "\u{e8fd}",
                Page::Feedback => "\u{e7f4}",
            };
            text(
                dc,
                icon,
                bounds(10, 0, 33, 38),
                16,
                Face::Icon,
                color,
                DT_CENTER,
            );
            text(
                dc,
                page.title(),
                bounds(41, 0, 139, 38),
                14,
                if selected { Face::Strong } else { Face::Body },
                color,
                0,
            );
        } else if id == THEME {
            fill(dc, r, p.surface);
            panel(dc, r, p.base, p.edge, 6);
            let icon = if DARK.with(Cell::get) {
                "\u{e706}"
            } else {
                "\u{e708}"
            };
            text(
                dc,
                icon,
                bounds(7, 0, 29, 34),
                14,
                Face::Icon,
                p.muted,
                DT_CENTER,
            );
            text(
                dc,
                &format!("切换至{}", caption(item.hwndItem)),
                bounds(35, 0, 132, 34),
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
