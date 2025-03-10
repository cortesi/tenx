use termimad::{crossterm::style::*, Alignment, MadSkin};

pub fn make_skin() -> MadSkin {
    let mut skin = MadSkin::default_dark();
    skin.set_headers_fg(Color::Cyan);
    skin.headers[0].compound_style.object_style.attributes = Attributes::default()
        .with(Attribute::Bold)
        .with(Attribute::DoubleUnderlined);
    for header in skin.headers.iter_mut().skip(1) {
        header.compound_style.object_style.attributes = Attributes::default().with(Attribute::Bold);
    }
    skin.headers[0].align = Alignment::Left;
    skin.table.align = Alignment::Left;
    skin
}
