use termimad::{crossterm::style::*, Alignment, MadSkin, *};

pub fn make_skin() -> MadSkin {
    let hdr_attrs = Attributes::default()
        .with(Attribute::Bold)
        .with(Attribute::Underlined);

    let mut skin = MadSkin::default();
    skin.table.align = Alignment::Center;
    skin.set_headers_fg(Color::Cyan);
    skin.code_block.align = Alignment::Center;
    skin.headers = [
        LineStyle {
            compound_style: CompoundStyle {
                object_style: ContentStyle {
                    foreground_color: Some(Color::Cyan),
                    background_color: None,
                    underline_color: None,
                    attributes: Attributes::default()
                        .with(Attribute::Bold)
                        .with(Attribute::DoubleUnderlined),
                },
            },
            align: Alignment::Unspecified,
            left_margin: 0,
            right_margin: 0,
        },
        LineStyle {
            compound_style: CompoundStyle {
                object_style: ContentStyle {
                    foreground_color: Some(Color::Cyan),
                    background_color: None,
                    underline_color: None,
                    attributes: hdr_attrs,
                },
            },
            align: Alignment::Unspecified,
            left_margin: 2,
            right_margin: 0,
        },
        LineStyle {
            compound_style: CompoundStyle {
                object_style: ContentStyle {
                    foreground_color: Some(Color::Cyan),
                    background_color: None,
                    underline_color: None,
                    attributes: hdr_attrs,
                },
            },
            align: Alignment::Unspecified,
            left_margin: 2,
            right_margin: 0,
        },
        LineStyle {
            compound_style: CompoundStyle {
                object_style: ContentStyle {
                    foreground_color: Some(Color::Cyan),
                    background_color: None,
                    underline_color: None,
                    attributes: hdr_attrs,
                },
            },
            align: Alignment::Unspecified,
            left_margin: 2,
            right_margin: 0,
        },
        LineStyle {
            compound_style: CompoundStyle {
                object_style: ContentStyle {
                    foreground_color: Some(Color::Cyan),
                    background_color: None,
                    underline_color: None,
                    attributes: hdr_attrs,
                },
            },
            align: Alignment::Unspecified,
            left_margin: 2,
            right_margin: 0,
        },
        LineStyle {
            compound_style: CompoundStyle {
                object_style: ContentStyle {
                    foreground_color: Some(Color::Cyan),
                    background_color: None,
                    underline_color: None,
                    attributes: hdr_attrs,
                },
            },
            align: Alignment::Unspecified,
            left_margin: 2,
            right_margin: 0,
        },
        LineStyle {
            compound_style: CompoundStyle {
                object_style: ContentStyle {
                    foreground_color: Some(Color::Cyan),
                    background_color: None,
                    underline_color: None,
                    attributes: hdr_attrs,
                },
            },
            align: Alignment::Unspecified,
            left_margin: 2,
            right_margin: 0,
        },
        LineStyle {
            compound_style: CompoundStyle {
                object_style: ContentStyle {
                    foreground_color: Some(Color::Cyan),
                    background_color: None,
                    underline_color: None,
                    attributes: hdr_attrs,
                },
            },
            align: Alignment::Unspecified,
            left_margin: 2,
            right_margin: 0,
        },
    ];

    skin
}
