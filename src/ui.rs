use super::*;
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED};

// Logical 96-DPI coordinates; Windows scales the entire window on high-DPI desktops.
pub const WIDTH: i32 = 812;
pub const HEIGHT: i32 = 552;
const SIDEBAR: i32 = 180;
const LEFT: i32 = 208;
const RIGHT: i32 = 784;
const TOP: i32 = 155;
const ROW: i32 = 48;

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
fn rgb(v: u32) -> u32 {
    (v >> 16) | (v & 0xff00) | ((v & 255) << 16)
}
fn colors() -> Palette {
    let [base, rail, surface, text, muted, accent, selected, edge] = if DARK.with(Cell::get) {
        [
            0x191d26, 0x141821, 0x242936, 0xe9edf6, 0xb3bdcd, 0xa4beff, 0x303f60, 0x354052,
        ]
    } else {
        [
            0xf5f6f9, 0xe9ecf2, 0xffffff, 0x202737, 0x546074, 0x2453b6, 0xe3ebfc, 0xdce2ed,
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
fn rect(x: i32, y: i32, r: i32, b: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: r,
        bottom: b,
    }
}
unsafe fn fill(dc: HDC, r: RECT, c: u32) {
    unsafe {
        SetDCBrushColor(dc, c);
        FillRect(dc, &r, GetStockObject(DC_BRUSH));
    }
}
unsafe fn rounded(dc: HDC, r: RECT, c: u32, radius: i32) {
    unsafe {
        SetDCBrushColor(dc, c);
        let brush = SelectObject(dc, GetStockObject(DC_BRUSH));
        let pen = SelectObject(dc, GetStockObject(NULL_PEN));
        RoundRect(
            dc,
            r.left,
            r.top,
            r.right + 1,
            r.bottom + 1,
            radius * 2,
            radius * 2,
        );
        SelectObject(dc, pen);
        SelectObject(dc, brush);
    }
}
unsafe fn text(dc: HDC, s: &str, r: RECT, size: i32, bold: bool, color: u32, align: u32) {
    unsafe {
        let font = CreateFontW(
            -size,
            0,
            0,
            0,
            if bold { 600 } else { 400 },
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            wide("Microsoft YaHei UI").as_ptr(),
        );
        let old = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        let mut area = r;
        let mut buf = wide(s);
        DrawTextW(
            dc,
            buf.as_mut_ptr(),
            -1,
            &mut area,
            DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | align,
        );
        SelectObject(dc, old);
        DeleteObject(font);
    }
}
// Leave input, accessibility and the popup list to the native combo box.
unsafe extern "system" fn action_field(
    w: HWND,
    m: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    unsafe {
        if m == WM_PAINT || m == WM_PRINTCLIENT || m == WM_PRINT {
            let mut ps = PAINTSTRUCT::default();
            let dc = if m == WM_PAINT {
                BeginPaint(w, &mut ps)
            } else {
                wp as HDC
            };
            let mut r = RECT::default();
            GetClientRect(w, &mut r);
            let p = colors();
            fill(dc, r, p.surface);
            rounded(dc, r, p.base, 5);
            let mut buf = [0u16; 80];
            let n = GetWindowTextW(w, buf.as_mut_ptr(), 80);
            text(
                dc,
                &String::from_utf16_lossy(&buf[..n as usize]),
                rect(12, 0, r.right - 32, r.bottom),
                15,
                false,
                p.text,
                0,
            );
            let cx = r.right - 16;
            let cy = r.bottom / 2;
            let pen = CreatePen(PS_SOLID, 2, p.muted);
            let old = SelectObject(dc, pen);
            MoveToEx(dc, cx - 4, cy - 2, null_mut());
            LineTo(dc, cx, cy + 2);
            LineTo(dc, cx + 5, cy - 3);
            SelectObject(dc, old);
            DeleteObject(pen);
            if windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus() == w {
                InflateRect(&mut r, -2, -2);
                DrawFocusRect(dc, &r);
            }
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
pub unsafe fn build() {
    unsafe {
        for (id, name, bounds) in [
            (SCOPE, "通用规则", [12, 104, 156, 40]),
            (BROWSER, "浏览器", [12, 150, 156, 40]),
            (THEME, "深色", [12, HEIGHT - 60, 156, 36]),
            (SAVE, "保存规则", [RIGHT - 104, 31, 104, 38]),
            (TOGGLE, "启用全局手势", [RIGHT - 240, 31, 124, 38]),
        ] {
            control("BUTTON", name, id, WS_TABSTOP | BS_OWNERDRAW as u32, bounds);
        }
        for i in 0..4 {
            let field = control(
                "COMBOBOX",
                "",
                FIRST + i,
                WS_TABSTOP
                    | CBS_DROPDOWNLIST as u32
                    | CBS_OWNERDRAWFIXED as u32
                    | CBS_HASSTRINGS as u32,
                [480, TOP + i * ROW + 9, 275, 300],
            );
            SetWindowSubclass(field, Some(action_field), 1, 0);
        }
        control(
            "GlideQuill.Practice",
            "试画区域",
            PAD,
            0,
            [LEFT, 411, RIGHT - LEFT, 76],
        );
        control(
            "STATIC",
            "已暂停 · 可在上方试画",
            RESULT,
            0,
            [LEFT, 501, RIGHT - LEFT, 29],
        );
    }
}
pub unsafe fn paint(w: HWND) {
    unsafe {
        let p = colors();
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(w, &mut ps);
        fill(dc, rect(0, 0, WIDTH, HEIGHT), p.base);
        fill(dc, rect(0, 0, SIDEBAR, HEIGHT), p.rail);
        text(dc, "GlideQuill", rect(22, 16, 170, 60), 25, true, p.text, 0);
        text(dc, "应用范围", rect(24, 70, 160, 98), 13, false, p.muted, 0);
        let browser = GROUP.with(Cell::get);
        text(
            dc,
            if browser { "浏览器" } else { "通用手势" },
            rect(LEFT, 21, 480, 69),
            30,
            true,
            p.text,
            0,
        );
        text(
            dc,
            if browser {
                "适用于 Chrome、Edge 和 Firefox"
            } else {
                "适用于普通窗口，浏览器可单独覆盖。"
            },
            rect(LEFT, 70, RIGHT, 101),
            15,
            false,
            p.muted,
            0,
        );
        text(
            dc,
            "手势",
            rect(LEFT + 16, 116, 400, 148),
            13,
            true,
            p.muted,
            0,
        );
        text(
            dc,
            "执行动作",
            rect(484, 116, 740, 148),
            13,
            true,
            p.muted,
            0,
        );
        for (i, (arrow, name)) in [
            ("←", "向左划"),
            ("→", "向右划"),
            ("↑", "向上划"),
            ("↓", "向下划"),
        ]
        .iter()
        .enumerate()
        {
            let y = TOP + i as i32 * ROW;
            rounded(dc, rect(LEFT, y, RIGHT, y + ROW - 4), p.surface, 8);
            text(
                dc,
                arrow,
                rect(LEFT + 18, y, LEFT + 50, y + 44),
                21,
                true,
                p.accent,
                0,
            );
            text(
                dc,
                name,
                rect(LEFT + 68, y, 440, y + 44),
                16,
                true,
                p.text,
                0,
            );
        }
        text(dc, "试画", rect(LEFT, 363, 300, 402), 19, true, p.text, 0);
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
            r.left += 8;
            text(dc, name, r, 15, false, p.text, 0);
        } else {
            let nav = id == SCOPE || id == BROWSER;
            let active = nav && (id == BROWSER) == GROUP.with(Cell::get);
            let rail = nav || id == THEME;
            fill(dc, r, if rail { p.rail } else { p.base });
            let bg = if id == SAVE {
                p.accent
            } else if active {
                p.selected
            } else if rail {
                p.rail
            } else {
                p.edge
            };
            rounded(dc, r, bg, 7);
            let mut buf = [0; 100];
            let n = GetWindowTextW(item.hwndItem, buf.as_mut_ptr(), 100);
            let mut name = String::from_utf16_lossy(&buf[..n as usize]);
            if id == THEME {
                name = format!("切换至{name}外观");
            }
            if rail {
                r.left += 14
            }
            text(
                dc,
                &name,
                r,
                15,
                active || id == SAVE,
                if id == SAVE {
                    p.base
                } else if active {
                    p.accent
                } else {
                    p.text
                },
                if rail { 0 } else { DT_CENTER },
            );
        }
        if item.itemState & ODS_FOCUS != 0 {
            let mut f = item.rcItem;
            InflateRect(&mut f, -3, -3);
            DrawFocusRect(dc, &f);
        }
    }
}
pub unsafe fn static_style(dc: HDC) -> LRESULT {
    unsafe {
        let p = colors();
        SetTextColor(dc, p.muted);
        SetBkColor(dc, p.base);
        SetDCBrushColor(dc, p.base);
        GetStockObject(DC_BRUSH) as isize
    }
}
pub unsafe fn pad_background(dc: HDC, r: RECT, empty: bool) {
    unsafe {
        let p = colors();
        fill(dc, r, p.base);
        rounded(dc, r, p.selected, 9);
        if empty {
            text(dc, "按住右键，在此试画", r, 16, false, p.accent, DT_CENTER);
        }
    }
}
