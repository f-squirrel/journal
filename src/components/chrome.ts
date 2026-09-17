/// The shell's fixed dimensions, in one place because they have to agree:
/// `SIDEBAR_WIDTH` is an icon rail — one 40px button plus its gutters — and
/// every column's header is `HEADER_HEIGHT` tall with a bottom border, so the
/// hairline runs unbroken across the sidebar, the list and the entry pane.
/// Change it here or the three fall out of line.
export const SIDEBAR_WIDTH = 56;
export const LIST_WIDTH = 320;
export const HEADER_HEIGHT = 56;
