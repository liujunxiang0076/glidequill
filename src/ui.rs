use super::*;
use std::time::Instant;
use windows_sys::Win32::Graphics::GdiPlus::*;
use windows_sys::Win32::UI::Controls::{
    BPBF_TOPDOWNDIB, BeginBufferedPaint, BufferedPaintInit, BufferedPaintUnInit, DRAWITEMSTRUCT,
    EndBufferedPaint, ODS_DISABLED, ODS_FOCUS, ODS_NOFOCUSRECT, ODS_SELECTED, WM_MOUSELEAVE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetFocus, IsWindowEnabled, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE,
    TRACKMOUSEEVENT, TrackMouseEvent,
};

// Layout is in device-independent pixels. Convert once at each drawing/Win32 boundary.
pub const WIDTH: i32 = 940;
pub const HEIGHT: i32 = 550;
#[derive(Clone, Copy)]
struct Layout {
    width: i32,
    height: i32,
    rail: i32,
    left: i32,
    right: i32,
    card_top: i32,
    row_start: i32,
    row_height: i32,
    column: i32,
    setting_height: i32,
    roomy: bool,
}
impl Layout {
    fn general_capacity(self) -> i32 {
        let header = self.group_header();
        ((self.height - self.card_top - 74 - 3 * header) / 45).clamp(1, 5)
    }
    fn group_header(self) -> i32 {
        if self.roomy { 24 } else { 18 }
    }
    fn visible_general(self) -> std::ops::Range<i32> {
        let capacity = self.general_capacity();
        let first = GENERAL_FIRST.get().min(5 - capacity);
        first..(first + capacity)
    }
    fn general_row(self, index: i32) -> [i32; 4] {
        let visible = self.visible_general();
        let header = self.group_header();
        let group = |i| match i {
            0 => 0,
            1 | 2 => 1,
            _ => 2,
        };
        let height =
            ((self.height - self.card_top - 74 - header * 3) / visible.len() as i32).clamp(38, 66);
        let scroll_width = if self.general_capacity() < 5 { 20 } else { 0 };
        [
            self.left,
            self.card_top
                + header * (group(index) - group(visible.start) + 1)
                + height * (index - visible.start),
            self.right - self.left - scroll_width,
            height,
        ]
    }
    const fn new(width: i32, height: i32) -> Self {
        let roomy = width >= 825 && height >= 480;
        let rail = if width < 690 {
            56
        } else if roomy {
            168
        } else {
            148
        };
        let gutter = if width < 740 {
            16
        } else if roomy {
            30
        } else {
            24
        };
        let left = rail + gutter;
        let right = width - gutter;
        let card_top = if height < 380 {
            76
        } else if roomy {
            100
        } else {
            88
        };
        let row_start = card_top + if roomy { 54 } else { 44 };
        let row_height = (height - row_start - if roomy { 88 } else { 76 }) / 4;
        Self {
            width,
            height,
            rail,
            left,
            right,
            card_top,
            row_start,
            row_height,
            column: (right - left - 2) / 2,
            setting_height: (height - card_top - if roomy { 88 } else { 72 }) / 3,
            roomy,
        }
    }
}
thread_local! {static LAYOUT: Cell<Layout> = const { Cell::new(Layout::new(WIDTH,HEIGHT)) };}
const GENERAL_SCROLL: i32 = 190;
thread_local! {static GENERAL_FIRST: Cell<i32> = const {Cell::new(0)};}
const GENERAL_IDS: [i32; 6] = [
    TOGGLE,
    FULLSCREEN,
    EXCLUSIONS,
    AUTOSTART,
    THEME_LIGHT,
    THEME_DARK,
];
fn general_index(id: i32) -> Option<i32> {
    GENERAL_IDS
        .iter()
        .position(|&item| item == id)
        .map(|i| i.min(4) as i32)
}
unsafe fn sync_general_scroll() {
    let l = LAYOUT.get();
    let first = l.visible_general().start;
    GENERAL_FIRST.set(first);
    let info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMax: 4,
        nPage: l.general_capacity() as u32,
        nPos: first,
        ..Default::default()
    };
    unsafe {
        windows_sys::Win32::UI::Controls::SetScrollInfo(
            GetDlgItem(app_window(), GENERAL_SCROLL),
            SB_CTL,
            &info,
            1,
        );
    }
}
pub unsafe fn scroll_general(command: i32, thumb: i32) -> bool {
    let l = LAYOUT.get();
    if PAGE.get() != Page::General || l.general_capacity() == 5 {
        return false;
    }
    let first = l.visible_general().start;
    let next = match command {
        SB_LINEUP => first - 1,
        SB_LINEDOWN => first + 1,
        SB_PAGEUP => first - l.general_capacity(),
        SB_PAGEDOWN => first + l.general_capacity(),
        SB_TOP => 0,
        SB_BOTTOM => 5,
        SB_THUMBPOSITION | SB_THUMBTRACK => thumb,
        _ => return false,
    }
    .clamp(0, 5 - l.general_capacity());
    if next != first {
        GENERAL_FIRST.set(next);
        unsafe {
            let w = app_window();
            for id in GENERAL_IDS {
                place(id, visible_on(id, Page::General), true);
            }
            sync_general_scroll();
            let dirty = bounds(l.left, l.card_top, l.right, l.height - 60);
            RedrawWindow(w, &dirty, null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN);
        }
    }
    true
}
pub unsafe fn measure(w: HWND) {
    let mut r = RECT::default();
    unsafe {
        GetClientRect(w, &mut r);
    }
    let dpi = DPI.with(Cell::get) as i32;
    LAYOUT.with(|v| {
        v.set(Layout::new(
            (r.right * 96 + dpi / 2) / dpi,
            (r.bottom * 96 + dpi / 2) / dpi,
        ))
    });
}
pub const PICKER: i32 = 160;
const COUNT: i32 = GESTURES.len() as i32;
pub const RULE_ROW: i32 = 140;
thread_local! { static DPI: Cell<u32> = const { Cell::new(96) }; }
pub fn set_dpi(dpi: u32) {
    let dpi = if dpi == 0 { 96 } else { dpi };
    if DPI.replace(dpi) != dpi {
        FONTS.with_borrow_mut(Vec::clear);
    }
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

// Interpolate COLORREF channels independently in either theme.
fn mix(a: u32, b: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let from = ((a >> shift) & 255) as f32;
        let to = ((b >> shift) & 255) as f32;
        ((from + (to - from) * amount).round() as u32) << shift
    };
    channel(0) | channel(8) | channel(16)
}
#[derive(Clone, Copy)]
enum PointerState {
    Idle,
    Hover,
    Pressed,
}
impl PointerState {
    fn tint(self, base: u32, toward: u32) -> u32 {
        match self {
            Self::Idle => base,
            Self::Hover => mix(base, toward, 0.13),
            Self::Pressed => mix(base, toward, 0.28),
        }
    }
}

// Keep GDI+ active for the lifetime of the native window and its child controls.
pub struct GraphicsSession {
    gdiplus: usize,
    buffered: bool,
}
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
        Self {
            gdiplus: token,
            buffered: unsafe { BufferedPaintInit() >= 0 },
        }
    }
}
impl Drop for GraphicsSession {
    fn drop(&mut self) {
        if self.buffered {
            unsafe { BufferedPaintUnInit() };
        }
        if self.gdiplus != 0 {
            unsafe { GdiplusShutdown(self.gdiplus) };
        }
    }
}
// Composite each invalid region once. Drawing GDI+ shapes directly to the
// display DC forces repeated transfers between GDI and the window surface.
struct PaintBuffer {
    handle: isize,
    dc: HDC,
}
impl PaintBuffer {
    unsafe fn new(target: HDC, bounds: &RECT) -> Self {
        let mut dc = null_mut();
        let handle = unsafe {
            BeginBufferedPaint(target, bounds, BPBF_TOPDOWNDIB, std::ptr::null(), &mut dc)
        };
        Self {
            handle,
            dc: if handle == 0 { target } else { dc },
        }
    }
}
impl Drop for PaintBuffer {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe { EndBufferedPaint(self.handle, 1) };
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
// A dot marks the start. Rotate the same continuous U or L path into the
// axes used by the recognizer, including the arrowhead on the outgoing leg.
unsafe fn draw_turn(dc: HDC, tile: RECT, incoming: (f64, f64), outgoing: (f64, f64), color: u32) {
    let returning = incoming.0 == -outgoing.0 && incoming.1 == -outgoing.1;
    let across = if returning {
        (-incoming.1, incoming.0)
    } else {
        outgoing
    };
    let scale = (tile.right - tile.left) as f32 / 32.0;
    let point = |u: f32, v: f32| PointF {
        X: (tile.left + tile.right) as f32 * 0.5
            + scale * (u * across.0 as f32 + v * incoming.0 as f32),
        Y: (tile.top + tile.bottom) as f32 * 0.5
            + scale * (u * across.1 as f32 + v * incoming.1 as f32),
    };
    let knots = if returning {
        [
            (-6., -8.),
            (-6., -4.),
            (-6., 0.),
            (-6., 2.),
            (-6., 11.),
            (6., 11.),
            (6., 2.),
            (6., 0.),
            (6., -4.),
            (6., -8.),
        ]
    } else {
        [
            (-7., -8.),
            (-7., -6.),
            (-7., -3.),
            (-7., 0.),
            (-7., 5.),
            (-5., 7.),
            (0., 7.),
            (3., 7.),
            (5., 7.),
            (9., 7.),
        ]
    };
    let path = knots.map(|(u, v)| point(u, v));
    let start = path[0];
    let end = path[path.len() - 1];
    let dx = outgoing.0 as f32;
    let dy = outgoing.1 as f32;
    let head = |side: f32| PointF {
        X: end.X + scale * (-3.5 * dx - side * dy),
        Y: end.Y + scale * (-3.5 * dy + side * dx),
    };
    unsafe {
        let mut g = null_mut();
        let mut pen = null_mut();
        let mut brush = null_mut();
        let ink = 0xff00_0000 | rgb(color);
        if GdipCreateFromHDC(dc, &mut g) == Ok
            && GdipCreatePen1(ink, 1.6 * scale, UnitPixel, &mut pen) == Ok
        {
            GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
            GdipSetPenLineCap197819(pen, LineCapRound, LineCapRound, DashCapRound);
            GdipSetPenLineJoin(pen, LineJoinRound);
            GdipDrawBeziers(g, pen, path.as_ptr(), path.len() as i32);
            let arrow = [head(-2.6), end, head(2.6)];
            GdipDrawLines(g, pen, arrow.as_ptr(), 3);
            if GdipCreateSolidFill(ink, &mut brush) == Ok {
                let radius = 2.0 * scale;
                GdipFillEllipse(
                    g,
                    brush.cast(),
                    start.X - radius,
                    start.Y - radius,
                    2.0 * radius,
                    2.0 * radius,
                );
            }
        }
        if !brush.is_null() {
            GdipDeleteBrush(brush.cast());
        }
        if !pen.is_null() {
            GdipDeletePen(pen);
        }
        if !g.is_null() {
            GdipDeleteGraphics(g);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Face {
    Body,
    Strong,
    Arrow,
    Icon,
}

struct UiFont {
    face: Face,
    height: i32,
    handle: HFONT,
}
impl Drop for UiFont {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.handle);
        }
    }
}
thread_local! {
    static FONTS:RefCell<Vec<UiFont>>=const {RefCell::new(Vec::new())};
}
fn font(face: Face, size: i32) -> HFONT {
    let height = -px(size);
    FONTS.with_borrow_mut(|cache| {
        if let Some(font) = cache.iter().find(|f| f.face == face && f.height == height) {
            return font.handle;
        }
        let family = match face {
            Face::Icon => "Segoe MDL2 Assets",
            Face::Arrow => "Segoe UI Symbol",
            _ => "Microsoft YaHei UI",
        };
        let weight = if face == Face::Strong { 600 } else { 400 };
        let handle = unsafe {
            CreateFontW(
                height,
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
            )
        };
        if !handle.is_null() {
            cache.push(UiFont {
                face,
                height,
                handle,
            });
        }
        handle
    })
}
unsafe fn text(dc: HDC, s: &str, mut area: RECT, size: i32, face: Face, color: u32, align: u32) {
    unsafe {
        let old = SelectObject(dc, font(face, size));
        let mode = SetBkMode(dc, TRANSPARENT as i32);
        let ink = SetTextColor(dc, color);
        DrawTextW(
            dc,
            wide(s).as_mut_ptr(),
            -1,
            &mut area,
            DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX | align,
        );
        SetTextColor(dc, ink);
        SetBkMode(dc, mode);
        SelectObject(dc, old);
    }
}
fn control_bounds(id: i32) -> [i32; 4] {
    let l = LAYOUT.with(Cell::get);
    if let Some(index) = Page::ALL.iter().position(|page| *page as i32 == id) {
        let (top, height, gap) = if l.roomy { (22, 44, 7) } else { (16, 38, 5) };
        return [10, top + index as i32 * (height + gap), l.rail - 20, height];
    }
    match id {
        TOGGLE if PAGE.get() == Page::General => l.general_row(0),
        GENERAL_SCROLL => [
            l.right - 14,
            l.card_top,
            14,
            (l.height - l.card_top - 74).max(20),
        ],
        FULLSCREEN => l.general_row(1),
        EXCLUSIONS => l.general_row(2),
        AUTOSTART => l.general_row(3),
        THEME_LIGHT | THEME_DARK => {
            let [x, y, width, h] = l.general_row(4);
            let end = x + width - 12;
            [
                end - if id == THEME_LIGHT { 120 } else { 60 },
                y + (h - 28) / 2,
                60,
                28,
            ]
        }
        ADD_APP => [l.left, l.card_top, 100, 32],
        REMOVE_APP => [l.left + 110, l.card_top, 76, 32],
        BACK => [l.right - 115, 20, 115, 32],
        APP_LIST => [
            l.left,
            l.card_top + 44,
            l.right - l.left,
            (l.height - l.card_top - 116).max(36),
        ],
        SAVE_APPS => [l.right - 100, l.height - 51, 100, 34],
        DIRTY_RULES => {
            let (y, h, space) = if l.roomy {
                (l.height - 58, 36, 212)
            } else {
                (l.height - 48, 32, 186)
            };
            [l.right - space - 94, y, 94, h]
        }
        TOGGLE => [l.right - 113, 20, 113, 34],
        BASIC_TAB if l.roomy => [l.left + 14, l.card_top + 9, 88, 36],
        SPECIAL_TAB if l.roomy => [l.left + 108, l.card_top + 9, 88, 36],
        BASIC_TAB => [l.left + 10, l.card_top + 7, 80, 30],
        SPECIAL_TAB => [l.left + 96, l.card_top + 7, 80, 30],
        RESET_RULES if l.roomy => [l.right - 204, l.height - 58, 78, 36],
        SAVE_RULES if l.roomy => [l.right - 116, l.height - 58, 116, 36],
        RESET_RULES => [l.right - 180, l.height - 48, 68, 32],
        SAVE_RULES => [l.right - 102, l.height - 48, 102, 32],
        IDLE => [
            l.right - 145,
            l.card_top + (l.setting_height - 34) / 2,
            132,
            34,
        ],
        LINGER => [
            l.right - 145,
            l.card_top + l.setting_height + (l.setting_height - 34) / 2,
            132,
            34,
        ],
        RESULT => [
            l.left,
            if l.roomy {
                l.height - 58
            } else {
                l.height - 48
            },
            l.right
                - l.left
                - if PAGE.with(Cell::get) == Page::Rules {
                    if l.roomy { 312 } else { 284 }
                } else if PAGE.get() == Page::Exclusions {
                    108
                } else {
                    0
                },
            if l.roomy { 36 } else { 32 },
        ],
        _ => {
            let picker = id >= PICKER;
            let index = (id - if picker { PICKER } else { RULE_ROW }) % BASIC_COUNT as i32;
            let x = l.left + 1 + index / 4 * l.column;
            let y = l.row_start + index % 4 * l.row_height;
            let (label, height, gap) = if l.roomy {
                (132, 36, 18)
            } else {
                (112, 30, 14)
            };
            if picker {
                [
                    x + label,
                    y + (l.row_height - height) / 2,
                    l.column - label - gap,
                    330,
                ]
            } else {
                [x + 1, y + 1, label - 3, l.row_height - 2]
            }
        }
    }
}
fn controls() -> impl Iterator<Item = i32> {
    [
        THEME_DARK,
        THEME_LIGHT,
        FULLSCREEN,
        EXCLUSIONS,
        ADD_APP,
        REMOVE_APP,
        SAVE_APPS,
        BACK,
        APP_LIST,
        DIRTY_RULES,
        GENERAL_SCROLL,
        TOGGLE,
        AUTOSTART,
        IDLE,
        LINGER,
        RESULT,
        BASIC_TAB,
        SPECIAL_TAB,
        RESET_RULES,
        SAVE_RULES,
    ]
    .into_iter()
    .chain(Page::ALL.map(|p| p as i32))
    .chain(RULE_ROW..RULE_ROW + COUNT)
    .chain(PICKER..PICKER + COUNT)
}
fn visible_on(id: i32, page: Page) -> bool {
    if page == Page::General
        && let Some(index) = general_index(id)
    {
        return LAYOUT.get().visible_general().contains(&index);
    }
    match id {
        GENERAL_SCROLL => page == Page::General && LAYOUT.get().general_capacity() < 5,
        THEME_LIGHT | THEME_DARK | FULLSCREEN | EXCLUSIONS | AUTOSTART => false,
        IDLE | LINGER => page == Page::Feedback,
        ADD_APP | REMOVE_APP | SAVE_APPS | BACK | APP_LIST => page == Page::Exclusions,
        DIRTY_RULES => page == Page::Rules,
        TOGGLE => matches!(page, Page::Rules | Page::General),
        RESET_RULES | SAVE_RULES | BASIC_TAB | SPECIAL_TAB => page == Page::Rules,
        id if (RULE_ROW..RULE_ROW + COUNT).contains(&id) => {
            page == Page::Rules && rule_range().contains(&((id - RULE_ROW) as usize))
        }
        id if (PICKER..PICKER + COUNT).contains(&id) => {
            page == Page::Rules && rule_range().contains(&((id - PICKER) as usize))
        }
        _ => true,
    }
}
// Geometry changes on resize/DPI. Navigation changes visibility and only moves
// the two controls shared between pages at different positions.
unsafe fn place(id: i32, visible: bool, move_control: bool) {
    let [x, y, width, height] = control_bounds(id).map(px);
    let visibility = if visible {
        SWP_SHOWWINDOW
    } else {
        SWP_HIDEWINDOW
    };
    let geometry = if move_control {
        0
    } else {
        SWP_NOMOVE | SWP_NOSIZE
    };
    unsafe {
        SetWindowPos(
            GetDlgItem(app_window(), id),
            null_mut(),
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW | visibility | geometry,
        );
    }
}
pub unsafe fn layout() {
    unsafe {
        let page = PAGE.get();
        sync_general_scroll();
        for id in controls() {
            place(id, visible_on(id, page), true);
            if (PICKER..PICKER + COUNT).contains(&id) {
                let field = GetDlgItem(app_window(), id);
                SendMessageW(
                    field,
                    CB_SETITEMHEIGHT,
                    0,
                    px(if LAYOUT.get().roomy { 32 } else { 28 }) as isize,
                );
                SendMessageW(
                    field,
                    CB_SETITEMHEIGHT,
                    usize::MAX,
                    px(if LAYOUT.get().roomy { 32 } else { 26 }) as isize,
                );
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
pub unsafe fn switch_page(previous: Page, page: Page) {
    unsafe {
        let w = app_window();
        for id in controls() {
            let visible = visible_on(id, page);
            let moves = matches!(id, TOGGLE | RESULT);
            if visible != visible_on(id, previous) || moves {
                place(id, visible, moves);
            }
        }
        let l = LAYOUT.get();
        let content = bounds(l.rail, 0, l.width, l.height);
        RedrawWindow(w, &content, null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN);
        for changed in [previous, page].map(|p| {
            if p == Page::Exclusions {
                Page::General
            } else {
                p
            }
        }) {
            InvalidateRect(GetDlgItem(w, changed as i32), null_mut(), 0);
        }
    }
}
pub unsafe fn switch_group() {
    unsafe {
        let w = app_window();
        let visible = rule_range();
        for index in 0..GESTURES.len() {
            let flag = if visible.contains(&index) {
                SWP_SHOWWINDOW
            } else {
                SWP_HIDEWINDOW
            };
            for base in [RULE_ROW, PICKER] {
                SetWindowPos(
                    GetDlgItem(w, base + index as i32),
                    null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW | flag,
                );
            }
        }
        let l = LAYOUT.with(Cell::get);
        // The description, tabs and rules change together; the sidebar and footer stay put.
        let dirty = bounds(l.left, 52, l.right, l.row_start + 4 * l.row_height + 1);
        RedrawWindow(w, &dirty, null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN);
    }
}
pub unsafe fn build() {
    unsafe {
        for (id, name) in [
            (THEME_LIGHT, "浅色"),
            (THEME_DARK, "深色"),
            (
                FULLSCREEN,
                if settings::preferences().pause_fullscreen {
                    "全屏时暂停手势 · 已开启"
                } else {
                    "全屏时暂停手势 · 已关闭"
                },
            ),
            (EXCLUSIONS, "排除的应用"),
            (ADD_APP, "添加应用…"),
            (REMOVE_APP, "移除"),
            (SAVE_APPS, "保存"),
            (BACK, "返回常规设置"),
            (Page::General as i32, Page::General.title()),
            (Page::Rules as i32, Page::Rules.title()),
            (Page::Feedback as i32, Page::Feedback.title()),
            (TOGGLE, "启用全局手势"),
            (BASIC_TAB, "基础方向"),
            (SPECIAL_TAB, "特殊手势"),
            (RESET_RULES, "重置本组"),
            (SAVE_RULES, "保存并重载"),
            (AUTOSTART, "启用登录自启"),
            (IDLE, "停笔取消时间"),
            (LINGER, "松键后保留时间"),
        ] {
            let button = control(
                "BUTTON",
                name,
                id,
                WS_TABSTOP | BS_OWNERDRAW as u32,
                control_bounds(id),
            );
            SetWindowSubclass(button, Some(interactive_control), 2, id as usize);
        }
        control(
            "SCROLLBAR",
            "滚动常规设置",
            GENERAL_SCROLL,
            WS_TABSTOP | SBS_VERT as u32,
            control_bounds(GENERAL_SCROLL),
        );
        let static_style = windows_sys::Win32::System::SystemServices::SS_OWNERDRAW;
        label(
            EXCLUSIONS,
            &format!("排除的应用 · {} 个应用", settings::exclusions().len()),
        );
        control(
            "LISTBOX",
            "排除的应用",
            APP_LIST,
            WS_TABSTOP
                | WS_BORDER
                | WS_VSCROLL
                | LBS_NOTIFY as u32
                | LBS_NOINTEGRALHEIGHT as u32
                | LBS_HASSTRINGS as u32
                | LBS_OWNERDRAWFIXED as u32,
            control_bounds(APP_LIST),
        );
        control(
            "STATIC",
            "",
            DIRTY_RULES,
            static_style,
            control_bounds(DIRTY_RULES),
        );
        control(
            "STATIC",
            "已暂停 · 开启全局手势即可显示划动轨迹",
            RESULT,
            static_style,
            control_bounds(RESULT),
        );
        for (index, dir) in GESTURES.iter().enumerate() {
            let id = RULE_ROW + index as i32;
            let row = control(
                "STATIC",
                dir.name(),
                id,
                static_style | windows_sys::Win32::System::SystemServices::SS_NOTIFY,
                control_bounds(id),
            );
            SetWindowSubclass(row, Some(interactive_control), 2, id as usize);
            let id = PICKER + index as i32;
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
            for action in ACTIONS {
                SendMessageW(
                    field,
                    CB_ADDSTRING,
                    0,
                    wide(action.label()).as_ptr() as isize,
                );
            }
            SendMessageW(
                field,
                CB_SETCURSEL,
                settings::preferences().bindings[index] as usize,
                0,
            );
            SetWindowSubclass(field, Some(action_field), 1, 0);
            SetWindowSubclass(field, Some(interactive_control), 2, id as usize);
        }
    }
}
unsafe fn caption(w: HWND) -> String {
    let mut buf = [0; 256];
    let n = unsafe { GetWindowTextW(w, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n as usize])
}
thread_local! {
    static HOT_CONTROL: Cell<HWND> = const { Cell::new(null_mut()) };
    static PRESSED_CLEAR: Cell<HWND> = const { Cell::new(null_mut()) };
}
const SLIDE_TIMER: usize = 6;
const SLIDE_SECONDS: f32 = 0.16;
#[derive(Clone, Copy)]
struct Slide {
    start: Instant,
    from: f32,
    to: f32,
}
impl Slide {
    fn position(self) -> f32 {
        let t = (self.start.elapsed().as_secs_f32() / SLIDE_SECONDS).min(1.0);
        self.from + (self.to - self.from) * (1.0 - (1.0 - t) * (1.0 - t))
    }
}
thread_local! { static SLIDES: [Cell<Option<Slide>>;3] = const {[const { Cell::new(None) };3]}; }
fn slide_slot(id: i32) -> usize {
    match id {
        AUTOSTART => 1,
        FULLSCREEN => 2,
        _ => 0,
    }
}
fn switch_position(id: i32, on: bool) -> f32 {
    SLIDES
        .with(|slots| slots[slide_slot(id)].get())
        .map_or(f32::from(on), Slide::position)
}
pub fn animate_switch(id: i32, was: bool, on: bool) {
    if was == on {
        return;
    }
    unsafe {
        let w = GetDlgItem(app_window(), id);
        let mut motion: i32 = 0;
        if IsWindowVisible(w) == 0
            || SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut motion as *mut i32).cast(),
                0,
            ) == 0
            || motion == 0
        {
            stop_slide(w, id);
            return;
        }
        let slide = Slide {
            start: Instant::now(),
            from: switch_position(id, was),
            to: f32::from(on),
        };
        if SetTimer(w, SLIDE_TIMER, 16, None) != 0 {
            SLIDES.with(|slots| slots[slide_slot(id)].set(Some(slide)));
            InvalidateRect(w, null_mut(), 0);
        }
    }
}
unsafe fn stop_slide(w: HWND, id: i32) {
    unsafe {
        KillTimer(w, SLIDE_TIMER);
    }
    SLIDES.with(|slots| slots[slide_slot(id)].set(None));
}
fn adjacent_control(id: i32, key: usize) -> Option<i32> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_LEFT, VK_RIGHT, VK_UP};
    if matches!(id, BASIC_TAB | SPECIAL_TAB) && matches!(key as u16, VK_LEFT | VK_RIGHT) {
        return Some(if id == BASIC_TAB {
            SPECIAL_TAB
        } else {
            BASIC_TAB
        });
    }
    let index = Page::ALL.iter().position(|p| *p as i32 == id)?;
    let len = Page::ALL.len();
    let next = match key as u16 {
        VK_UP => (index + len - 1) % len,
        VK_DOWN => (index + 1) % len,
        _ => return None,
    };
    Some(Page::ALL[next] as i32)
}
unsafe extern "system" fn interactive_control(
    w: HWND,
    m: u32,
    wp: WPARAM,
    lp: LPARAM,
    _subclass: usize,
    data: usize,
) -> LRESULT {
    unsafe {
        let id = data as i32;
        match m {
            WM_MOUSEMOVE if HOT_CONTROL.with(Cell::get) != w => {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: w,
                    ..Default::default()
                };
                if TrackMouseEvent(&mut track) != 0 {
                    let previous = HOT_CONTROL.with(|v| v.replace(w));
                    if !previous.is_null() {
                        InvalidateRect(previous, null_mut(), 0);
                    }
                    InvalidateRect(w, null_mut(), 0);
                }
            }
            WM_MOUSELEAVE if HOT_CONTROL.with(Cell::get) == w => {
                HOT_CONTROL.with(|v| v.set(null_mut()));
                InvalidateRect(w, null_mut(), 0);
            }
            WM_SHOWWINDOW if wp == 0 => {
                if HOT_CONTROL.with(Cell::get) == w {
                    HOT_CONTROL.with(|hot| hot.set(null_mut()));
                }
                if matches!(id, TOGGLE | AUTOSTART | FULLSCREEN) {
                    stop_slide(w, id);
                }
            }
            WM_TIMER if wp == SLIDE_TIMER && matches!(id, TOGGLE | AUTOSTART | FULLSCREEN) => {
                let slide = SLIDES.with(|slots| slots[slide_slot(id)].get());
                if slide.is_none_or(|s| s.start.elapsed().as_secs_f32() >= SLIDE_SECONDS) {
                    stop_slide(w, id);
                }
                InvalidateRect(w, null_mut(), 0);
                return 0;
            }
            WM_GETDLGCODE if adjacent_control(id, wp).is_some() => {
                return DefSubclassProc(w, m, wp, lp) | DLGC_WANTARROWS as isize;
            }
            WM_KEYDOWN => {
                let Some(next_id) = adjacent_control(id, wp) else {
                    return DefSubclassProc(w, m, wp, lp);
                };
                let next = GetDlgItem(app_window(), next_id);
                SetFocus(next);
                SendMessageW(next, BM_CLICK, 0, 0);
                return 0;
            }
            WM_SETCURSOR if IsWindowEnabled(w) != 0 && (lp as u16) == HTCLIENT as u16 => {
                SetCursor(LoadCursorW(null_mut(), IDC_HAND));
                return 1;
            }
            _ => {}
        }
        DefSubclassProc(w, m, wp, lp)
    }
}
unsafe fn clear_target(w: HWND) -> Option<RECT> {
    unsafe {
        if HOT_CONTROL.with(Cell::get) != w
            || IsWindowEnabled(w) == 0
            || SendMessageW(w, CB_GETCURSEL, 0, 0) <= Action::Unassigned as isize
            || SendMessageW(w, CB_GETDROPPEDSTATE, 0, 0) != 0
        {
            return None;
        }
        let mut r = RECT::default();
        GetClientRect(w, &mut r);
        r.left = r.right - px(46);
        r.right -= px(23);
        Some(r)
    }
}
unsafe fn hits_clear(w: HWND, lp: LPARAM) -> bool {
    let pt = POINT {
        x: lp as i16 as i32,
        y: (lp >> 16) as i16 as i32,
    };
    unsafe { clear_target(w).is_some_and(|r| PtInRect(&r, pt) != 0) }
}
// Only the clear target intercepts clicks. The rest of the field stays a native combo.
unsafe extern "system" fn action_field(
    w: HWND,
    m: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    unsafe {
        match m {
            WM_LBUTTONDOWN if hits_clear(w, lp) => {
                SetFocus(w);
                SetCapture(w);
                PRESSED_CLEAR.with(|v| v.set(w));
                InvalidateRect(w, null_mut(), 0);
                return 0;
            }
            WM_LBUTTONUP if PRESSED_CLEAR.with(Cell::get) == w => {
                let clear = hits_clear(w, lp);
                PRESSED_CLEAR.with(|v| v.set(null_mut()));
                if GetCapture() == w {
                    ReleaseCapture();
                }
                if clear {
                    SendMessageW(w, CB_SETCURSEL, Action::Unassigned as usize, 0);
                    update_rule_buttons();
                }
                InvalidateRect(w, null_mut(), 0);
                return 0;
            }
            WM_CAPTURECHANGED | WM_CANCELMODE if PRESSED_CLEAR.with(Cell::get) == w => {
                PRESSED_CLEAR.with(|v| v.set(null_mut()));
                InvalidateRect(w, null_mut(), 0);
            }
            _ => {}
        }
        if matches!(m, WM_PAINT | WM_PRINTCLIENT | WM_PRINT) {
            let mut ps = PAINTSTRUCT::default();
            let dc = if m == WM_PAINT {
                BeginPaint(w, &mut ps)
            } else {
                wp as HDC
            };
            let mut r = RECT::default();
            GetClientRect(w, &mut r);
            let buffer = PaintBuffer::new(dc, &r);
            let dc = buffer.dc;
            let p = colors();
            let focus = GetFocus() == w;
            fill(dc, r, p.surface);
            let hot = HOT_CONTROL.with(Cell::get) == w;
            let open = SendMessageW(w, CB_GETDROPPEDSTATE, 0, 0) != 0;
            let border = if focus || open {
                p.accent
            } else if hot {
                mix(p.edge, p.accent, 0.55)
            } else {
                p.edge
            };
            panel(dc, r, p.surface, border, 5);
            let chevron = RECT {
                left: r.right - px(23),
                ..r
            };
            text(
                dc,
                if open { "\u{e70e}" } else { "\u{e70d}" },
                chevron,
                11,
                Face::Icon,
                p.muted,
                DT_CENTER,
            );
            r.left += px(9);
            r.right = if let Some(clear) = clear_target(w) {
                if PRESSED_CLEAR.with(Cell::get) == w {
                    rounded(dc, inset(clear, px(3)), p.selected, 4);
                }
                text(dc, "\u{e711}", clear, 10, Face::Icon, p.muted, DT_CENTER);
                clear.left
            } else {
                chevron.left
            };
            text(dc, &caption(w), r, 14, Face::Body, p.text, 0);
            drop(buffer);
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
pub unsafe fn list_background(dc: HDC) -> HBRUSH {
    let surface = colors().surface;
    unsafe {
        SetBkColor(dc, surface);
        SetDCBrushColor(dc, surface);
        GetStockObject(DC_BRUSH)
    }
}
pub unsafe fn paint(w: HWND) {
    unsafe {
        let p = colors();
        let l = LAYOUT.with(Cell::get);
        let mut ps = PAINTSTRUCT::default();
        let target = BeginPaint(w, &mut ps);
        let buffer = PaintBuffer::new(target, &ps.rcPaint);
        let dc = buffer.dc;
        fill(dc, bounds(0, 0, l.width, l.height), p.base);
        fill(dc, bounds(0, 0, l.rail, l.height), p.rail);
        fill(dc, bounds(l.rail - 1, 0, l.rail, l.height), p.edge);
        let page = PAGE.with(Cell::get);
        text(
            dc,
            page.title(),
            bounds(l.left, 14, l.right - 120, 55),
            23,
            Face::Strong,
            p.text,
            0,
        );
        let description = match page {
            Page::Rules if SPECIAL_GROUP.with(Cell::get) => {
                "按住右键连续回划或转向，松开执行已绑定的动作。"
            }
            Page::Rules => "松开右键执行绑定动作。修改绑定后点击「保存并重载」。",
            Page::General => "管理手势的运行状态、生效范围与窗口外观。",
            Page::Exclusions => "按程序文件名匹配，同名程序共用一条排除规则。",
            Page::Feedback => "调整停笔和松键后，轨迹与动作字幕的显示时间。",
        };
        text(
            dc,
            description,
            bounds(l.left, 54, l.right, 80),
            13,
            Face::Body,
            p.muted,
            0,
        );
        if page == Page::Rules {
            panel(
                dc,
                bounds(
                    l.left,
                    l.card_top,
                    l.right,
                    l.row_start + l.row_height * 4 + 1,
                ),
                p.surface,
                p.edge,
                8,
            );
            text(
                dc,
                "绑定动作",
                bounds(l.right - 145, l.card_top, l.right - 13, l.row_start),
                12,
                Face::Body,
                p.muted,
                DT_RIGHT,
            );
            for row in 0..4 {
                let y = l.row_start + row * l.row_height;
                fill(dc, bounds(l.left + 1, y, l.right - 1, y + 1), p.edge);
            }
            fill(
                dc,
                bounds(
                    l.left + 1 + l.column,
                    l.row_start + 1,
                    l.left + 2 + l.column,
                    l.row_start + 4 * l.row_height,
                ),
                p.edge,
            );
        } else if page == Page::General {
            let visible = l.visible_general();
            for index in visible.clone() {
                if index != visible.start && !matches!(index, 1 | 3) {
                    continue;
                }
                let heading = match index {
                    0 => "手势",
                    1 | 2 => "生效范围",
                    _ => "启动与外观",
                };
                let [x, y, _, _] = l.general_row(index);
                text(
                    dc,
                    heading,
                    bounds(x, y - if l.roomy { 24 } else { 18 }, l.right, y),
                    12,
                    Face::Strong,
                    p.muted,
                    0,
                );
            }
            if visible.contains(&4) {
                let [x, y, width, h] = l.general_row(4);
                rounded(dc, bounds(x, y, x + width, y + h - 1), p.surface, 6);
                text(
                    dc,
                    "窗口外观",
                    bounds(x + 14, y, l.right - 140, y + h),
                    14,
                    Face::Strong,
                    p.text,
                    0,
                );
            }
        } else if page == Page::Feedback {
            let rows = [
                ("按住右键时", "停笔多久后取消本次手势并清除轨迹"),
                ("松开右键后", "轨迹和屏幕下方的动作名称保留多久"),
            ];
            panel(
                dc,
                bounds(
                    l.left,
                    l.card_top,
                    l.right,
                    l.card_top + l.setting_height * rows.len() as i32,
                ),
                p.surface,
                p.edge,
                8,
            );
            for (i, (name, desc)) in rows.iter().enumerate() {
                let y = l.card_top + i as i32 * l.setting_height;
                if i > 0 {
                    fill(dc, bounds(l.left + 16, y, l.right - 16, y + 1), p.edge);
                }
                let y = y + (l.setting_height - 64) / 2;
                text(
                    dc,
                    name,
                    bounds(l.left + 16, y + 8, l.right - 154, y + 35),
                    15,
                    Face::Strong,
                    p.text,
                    0,
                );
                text(
                    dc,
                    desc,
                    bounds(l.left + 16, y + 34, l.right - 154, y + 59),
                    12,
                    Face::Body,
                    p.muted,
                    0,
                );
            }
        }
        let divider = l.height - if l.roomy { 74 } else { 62 };
        fill(dc, bounds(l.left, divider, l.right, divider + 1), p.edge);
        drop(buffer);
        EndPaint(w, &ps);
    }
}
pub unsafe fn item(item: &DRAWITEMSTRUCT) {
    unsafe {
        let p = colors();
        let id = item.CtlID as i32;
        let r = item.rcItem;
        let buffer = PaintBuffer::new(item.hDC, &r);
        let dc = buffer.dc;
        let state = if item.itemState & ODS_DISABLED != 0 {
            PointerState::Idle
        } else if item.itemState & ODS_SELECTED != 0 {
            PointerState::Pressed
        } else if HOT_CONTROL.with(Cell::get) == item.hwndItem {
            PointerState::Hover
        } else {
            PointerState::Idle
        };
        if (RULE_ROW..RULE_ROW + COUNT).contains(&id) {
            let dir = GESTURES[(id - RULE_ROW) as usize];
            fill(dc, r, p.surface);
            let roomy = LAYOUT.get().roomy;
            let side = px(if roomy { 38 } else { 32 }).min(r.bottom - px(4));
            let start = px(if roomy { 14 } else { 10 });
            let top = (r.bottom - side) / 2;
            let tile = RECT {
                left: start,
                top,
                right: start + side,
                bottom: top + side,
            };
            rounded(dc, tile, state.tint(p.selected, p.accent), 7);
            if let Some((a, b)) = dir.legs() {
                draw_turn(dc, tile, a, b, p.accent);
            } else {
                text(dc, dir.arrow(), tile, 18, Face::Arrow, p.accent, DT_CENTER);
            }
            text(
                dc,
                dir.name(),
                RECT {
                    left: start + side + px(9),
                    ..r
                },
                14,
                Face::Body,
                p.text,
                0,
            );
        } else if (PICKER..PICKER + COUNT).contains(&id) {
            let selected = item.itemState & ODS_SELECTED != 0;
            fill(dc, r, if selected { p.selected } else { p.surface });
            if let Some(a) = ACTIONS.get(item.itemID as usize) {
                text(
                    dc,
                    a.label(),
                    RECT {
                        left: r.left + px(9),
                        ..r
                    },
                    14,
                    Face::Body,
                    p.text,
                    0,
                );
            }
        } else if id == BASIC_TAB || id == SPECIAL_TAB {
            let active = (id == SPECIAL_TAB) == SPECIAL_GROUP.with(Cell::get);
            fill(dc, r, p.surface);
            rounded(
                dc,
                r,
                state.tint(if active { p.selected } else { p.surface }, p.accent),
                5,
            );
            text(
                dc,
                &caption(item.hwndItem),
                r,
                13,
                if active { Face::Strong } else { Face::Body },
                if active { p.accent } else { p.muted },
                DT_CENTER,
            );
        } else if matches!(id, RESULT | DIRTY_RULES) {
            fill(dc, r, p.base);
            text(
                dc,
                &caption(item.hwndItem),
                r,
                12,
                Face::Body,
                if id == DIRTY_RULES { p.accent } else { p.muted },
                if id == DIRTY_RULES { DT_RIGHT } else { 0 },
            );
        } else if matches!(
            id,
            RESET_RULES | SAVE_RULES | ADD_APP | REMOVE_APP | SAVE_APPS | BACK
        ) {
            let enabled = IsWindowEnabled(item.hwndItem) != 0;
            let primary = enabled && matches!(id, SAVE_RULES | SAVE_APPS);
            fill(dc, r, p.base);
            let background = if primary {
                state.tint(p.accent, p.text)
            } else if enabled {
                state.tint(p.surface, p.accent)
            } else if matches!(id, SAVE_RULES | SAVE_APPS) {
                p.edge
            } else {
                p.base
            };
            panel(
                dc,
                r,
                background,
                if primary { p.accent } else { p.edge },
                6,
            );
            let ink = if primary {
                p.surface
            } else if enabled {
                p.text
            } else {
                p.muted
            };
            text(
                dc,
                &caption(item.hwndItem),
                r,
                13,
                Face::Body,
                ink,
                DT_CENTER,
            );
        } else if matches!(id, TOGGLE | AUTOSTART | FULLSCREEN | EXCLUSIONS) {
            let general = PAGE.get() == Page::General;
            let bg = if general {
                state.tint(p.surface, p.accent)
            } else {
                p.base
            };
            fill(dc, r, p.base);
            if general {
                rounded(
                    dc,
                    RECT {
                        bottom: r.bottom - 1,
                        ..r
                    },
                    bg,
                    6,
                );
                let (title, hint) = match id {
                    TOGGLE => ("全局手势", "按住右键划动，按住 Shift 可临时使用原生右键"),
                    FULLSCREEN => ("全屏时暂停手势", "退出全屏后自动恢复"),
                    EXCLUSIONS => ("排除的应用", "在指定程序中保留原生右键"),
                    _ => ("登录时自启", "登录 Windows 后在托盘运行"),
                };
                let left = RECT {
                    left: px(14),
                    right: r.right - px(132),
                    ..r
                };
                if r.bottom >= px(45) {
                    let middle = (r.bottom - r.top) / 2 + px(2);
                    text(
                        dc,
                        title,
                        RECT {
                            top: middle - px(23),
                            bottom: middle,
                            ..left
                        },
                        14,
                        Face::Strong,
                        p.text,
                        0,
                    );
                    text(
                        dc,
                        hint,
                        RECT {
                            top: middle,
                            bottom: middle + px(19),
                            ..left
                        },
                        12,
                        Face::Body,
                        p.muted,
                        0,
                    );
                } else {
                    text(dc, title, left, 14, Face::Strong, p.text, 0);
                }
            }
            if id == EXCLUSIONS {
                let name = caption(item.hwndItem);
                text(
                    dc,
                    &format!("{}  ›", name.split(" · ").last().unwrap_or("")),
                    RECT {
                        right: r.right - px(14),
                        ..r
                    },
                    14,
                    Face::Body,
                    p.accent,
                    DT_RIGHT,
                );
            } else {
                let on = match id {
                    AUTOSTART => STARTUP_ON.get(),
                    FULLSCREEN => settings::preferences().pause_fullscreen,
                    _ => LISTENER.with_borrow(|v| v.is_some()),
                };
                let progress = switch_position(id, on);
                let right = r.right - if general { px(14) } else { px(1) };
                let top = (r.bottom - px(20)) / 2;
                let track = RECT {
                    left: right - px(36),
                    top,
                    right,
                    bottom: top + px(20),
                };
                rounded(
                    dc,
                    track,
                    state.tint(mix(p.muted, p.accent, progress), p.text),
                    10,
                );
                let x = track.left + px(2) + (px(16) as f32 * progress).round() as i32;
                rounded(
                    dc,
                    RECT {
                        left: x,
                        top: top + px(2),
                        right: x + px(16),
                        bottom: top + px(18),
                    },
                    p.surface,
                    8,
                );
                let name = if !general {
                    "全局手势"
                } else if on {
                    "已开启"
                } else if id == TOGGLE {
                    "已暂停"
                } else {
                    "已关闭"
                };
                text(
                    dc,
                    name,
                    RECT {
                        right: track.left - px(10),
                        ..r
                    },
                    14,
                    Face::Body,
                    p.text,
                    DT_RIGHT,
                );
            }
        } else if id == IDLE || id == LINGER {
            fill(dc, r, p.surface);
            panel(
                dc,
                r,
                state.tint(p.base, p.accent),
                state.tint(p.edge, p.accent),
                6,
            );
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
            let selected =
                PAGE.get() == page || (page == Page::General && PAGE.get() == Page::Exclusions);
            fill(dc, r, p.rail);
            let background = if selected {
                state.tint(p.accent, p.text)
            } else {
                state.tint(p.rail, p.accent)
            };
            rounded(dc, r, background, 7);
            let color = if selected { p.surface } else { p.text };
            let icon = match page {
                Page::General | Page::Exclusions => "\u{e713}",
                Page::Rules => "\u{e8fd}",
                Page::Feedback => "\u{e7f4}",
            };
            text(
                dc,
                icon,
                RECT {
                    left: px(10),
                    right: px(33),
                    ..r
                },
                16,
                Face::Icon,
                color,
                DT_CENTER,
            );
            if LAYOUT.with(Cell::get).rail > 56 {
                text(
                    dc,
                    page.title(),
                    RECT { left: px(41), ..r },
                    14,
                    if selected { Face::Strong } else { Face::Body },
                    color,
                    0,
                );
            }
        } else if matches!(id, THEME_LIGHT | THEME_DARK) {
            let active = (id == THEME_DARK) == DARK.get();
            fill(dc, r, p.surface);
            panel(
                dc,
                r,
                state.tint(if active { p.selected } else { p.surface }, p.accent),
                if active { p.accent } else { p.edge },
                5,
            );
            text(
                dc,
                if id == THEME_DARK { "深色" } else { "浅色" },
                r,
                13,
                if active { Face::Strong } else { Face::Body },
                if active { p.accent } else { p.text },
                DT_CENTER,
            );
        } else if id == APP_LIST {
            let selected = item.itemState & ODS_SELECTED != 0;
            fill(dc, r, if selected { p.selected } else { p.surface });
            if item.itemID != u32::MAX {
                let len = SendMessageW(item.hwndItem, LB_GETTEXTLEN, item.itemID as usize, 0);
                if len >= 0 {
                    let mut buf = vec![0u16; len as usize + 1];
                    SendMessageW(
                        item.hwndItem,
                        LB_GETTEXT,
                        item.itemID as usize,
                        buf.as_mut_ptr() as isize,
                    );
                    text(
                        dc,
                        &String::from_utf16_lossy(&buf[..len as usize]),
                        RECT {
                            left: r.left + px(12),
                            ..r
                        },
                        14,
                        Face::Body,
                        p.text,
                        0,
                    );
                }
            }
        }
        if item.itemState & (ODS_FOCUS | ODS_NOFOCUSRECT) == ODS_FOCUS
            && SendMessageW(item.hwndItem, WM_QUERYUISTATE, 0, 0) & UISF_HIDEFOCUS as isize == 0
        {
            DrawFocusRect(dc, &inset(r, px(2)));
        }
    }
}
