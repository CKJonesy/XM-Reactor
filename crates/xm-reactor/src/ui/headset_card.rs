//! The headset card: render (or a headphone glyph), name, status, battery and now playing, or the
//! way back when the headphones are not connected.

use super::panel::{Panel, PanelMsg};
use crate::strings;
use crate::view_model::{Angle, PanelVm, RenderChoice};
use windows_reactor::*;
use xm_core::settings::HeadsetColour;

pub const BLUETOOTH_SETTINGS: &str = "ms-settings:bluetooth";
/// At or below this the battery bar takes the stock caution state.
const LOW_BATTERY: f64 = 0.2;

macro_rules! scales {
    ($colour:literal, $angle:literal) => {
        [
            scale!($colour, $angle, "1x"),
            scale!($colour, $angle, "1.5x"),
            scale!($colour, $angle, "2x"),
        ]
    };
}

macro_rules! scale {
    ($colour:literal, $angle:literal, $scale:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/renders/wh-1000xm6/",
            $colour,
            "-",
            $angle,
            "@",
            $scale,
            ".png"
        ))
        .as_slice()
    };
}

/// Cut-outs embedded in the exe, by colour (silver, blue, black), angle (front, three-quarter)
/// and scale (1x, 1.5x, 2x).
const RENDERS: [[[&[u8]; 3]; 2]; 3] = [
    [
        scales!("silver", "front"),
        scales!("silver", "three-quarter"),
    ],
    [scales!("blue", "front"), scales!("blue", "three-quarter")],
    [scales!("black", "front"), scales!("black", "three-quarter")],
];

/// The smallest cut-out scale that is at least the window's (0 = 1x, 1 = 1.5x, 2 = 2x).
pub fn scale_index(dpi_scale: f64) -> usize {
    if dpi_scale <= 1.0 {
        0
    } else if dpi_scale <= 1.5 {
        1
    } else {
        2
    }
}

pub fn render_png(choice: RenderChoice, dpi_scale: f64) -> &'static [u8] {
    let colour = match choice.colour {
        HeadsetColour::PlatinumSilver => 0,
        HeadsetColour::MidnightBlue => 1,
        HeadsetColour::Black => 2,
    };
    // Side and rear have no cut-outs, so they show the default three-quarter angle.
    let angle = match choice.angle {
        Angle::Front => 0,
        Angle::ThreeQuarter | Angle::Side | Angle::Rear => 1,
    };
    RENDERS[colour][angle][scale_index(dpi_scale)]
}

fn colour_name(c: HeadsetColour) -> &'static str {
    match c {
        HeadsetColour::PlatinumSilver => strings::COLOUR_SILVER,
        HeadsetColour::MidnightBlue => strings::COLOUR_BLUE,
        HeadsetColour::Black => strings::COLOUR_BLACK,
    }
}

/// The picture's accessible name: the device and the colour of the render shown.
pub fn picture_name(vm: &PanelVm) -> String {
    strings::headset_picture(&vm.device_name, vm.render.map(|r| colour_name(r.colour)))
}

const CARD_IMAGE: f64 = 96.0;
const HEADPHONE_GLYPH: &str = "\u{E7F6}";

/// Try again and the Bluetooth settings link, for the states where they help.
fn recovery(vm: &PanelVm, cx: &ViewContext<'_, Panel>) -> Vec<View> {
    let mut out = Vec::new();
    if vm.recovery.try_again {
        out.push(
            Button::new()
                .content(strings::TRY_AGAIN)
                .automation_name(strings::TRY_AGAIN)
                .margin(Thickness::new(0.0, 6.0, 0.0, 0.0))
                .on_click(cx.message(PanelMsg::TryAgain))
                .into(),
        );
    }
    if vm.recovery.open_bluetooth {
        // Opened by the panel through the shell rather than the link's own NavigateUri, which
        // did nothing in the installed app (owner, 2026-10-22); the shell also raises Settings.
        out.push(
            HyperlinkButton::new()
                .content(strings::OPEN_BLUETOOTH)
                .automation_name(strings::OPEN_BLUETOOTH)
                // Cancels the stock 11 DIP padding so the link text lines up with the status above.
                .margin(Thickness::new(-11.0, 0.0, 0.0, 0.0))
                .on_click(cx.message(PanelMsg::OpenBluetoothSettings))
                .into(),
        );
    }
    out
}

/// `scale` is the image's rasterisation scale, observed through `image`.
pub fn view(
    vm: &PanelVm,
    scale: f64,
    image: &ElementRef<Image>,
    cx: &ViewContext<'_, Panel>,
) -> View {
    let picture: View = match vm.render {
        Some(r) => Image::new()
            .element_ref(image)
            .source_data(EncodedImage::from_static(render_png(r, scale)))
            .stretch(Stretch::Uniform)
            .height(CARD_IMAGE)
            .automation_name(picture_name(vm))
            .into(),
        None => FontIcon::new()
            .glyph(HEADPHONE_GLYPH)
            .font_size(56.0)
            .height(CARD_IMAGE)
            .automation_name(strings::HEADSET_PICTURE)
            .into(),
    };
    let mut details: Vec<View> = vec![
        TextBlock::new()
            .text(&vm.device_name)
            .font_size(16.0)
            .font_weight(FontWeight::SEMI_BOLD)
            .max_lines(1)
            .text_trimming(TextTrimming::CharacterEllipsis)
            .tooltip(&vm.device_name),
        TextBlock::new()
            .text(&vm.status)
            .text_wrapping(TextWrapping::Wrap)
            .into(),
    ];
    // A reading kept from before a disconnect is stale, so it shows only while connected.
    if let (true, Some(text), Some(fraction)) =
        (vm.connected, &vm.battery_text, vm.battery_fraction)
    {
        let text = if vm.charging {
            strings::battery_charging(text)
        } else {
            text.clone()
        };
        details.push(
            ProgressBar::new()
                .maximum(100.0)
                .value(fraction * 100.0)
                .show_paused(fraction <= LOW_BATTERY && !vm.charging)
                .automation_name(strings::BATTERY_LEVEL)
                .margin(Thickness::new(0.0, 6.0, 0.0, 0.0))
                .into(),
        );
        details.push(TextBlock::new().text(text).font_size(12.0).into());
    }
    details.extend(recovery(vm, cx));
    if let Some(playing) = &vm.now_playing {
        details.push(
            TextBlock::new()
                .text(playing)
                .font_size(12.0)
                .max_lines(1)
                .text_trimming(TextTrimming::CharacterEllipsis)
                .tooltip(playing),
        );
    }
    Border::new()
        .background(ThemeBrush::CardBackground)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(1.0)
        .corner_radius(8.0)
        .padding(12.0)
        .content(
            Grid::new()
                .columns([GridLength::Pixel(CARD_IMAGE), GridLength::STAR])
                .column_spacing(12.0)
                .children((
                    picture,
                    StackPanel::new()
                        .spacing(2.0)
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(1)
                        .children(details),
                )),
        )
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(colour: HeadsetColour, angle: Angle) -> RenderChoice {
        RenderChoice { colour, angle }
    }

    #[test]
    fn scale_follows_dpi() {
        assert_eq!(scale_index(1.0), 0);
        assert_eq!(scale_index(1.25), 1);
        assert_eq!(scale_index(1.5), 1);
        assert_eq!(scale_index(1.75), 2);
        assert_eq!(scale_index(3.0), 2);
    }

    #[test]
    fn picture_is_named_from_the_device_and_colour() {
        use crate::view_model::{build, tests};
        use xm_core::session::SessionState;
        use xm_core::settings::Settings;
        let silver = xm_core::model::HeadphonesState {
            headset_colour: Some(HeadsetColour::PlatinumSilver),
            ..tests::ready()
        };
        let vm = build(&silver, &Settings::default(), false, false);
        assert_eq!(picture_name(&vm), "WH-1000XM6, Platinum Silver");
        // No render (not connected) shows the outline glyph, named plainly.
        let gone = build(
            &tests::base(SessionState::Disconnected),
            &Settings::default(),
            false,
            false,
        );
        assert_eq!(picture_name(&gone), "Headphones");
    }

    #[test]
    fn renders_are_pngs_and_side_falls_back_to_three_quarter() {
        let png = b"\x89PNG";
        for c in [
            HeadsetColour::PlatinumSilver,
            HeadsetColour::MidnightBlue,
            HeadsetColour::Black,
        ] {
            for s in [1.0, 1.5, 2.0] {
                let three = render_png(choice(c, Angle::ThreeQuarter), s);
                assert!(three.starts_with(png));
                assert_ne!(three, render_png(choice(c, Angle::Front), s));
                assert_eq!(three, render_png(choice(c, Angle::Side), s));
                assert_eq!(three, render_png(choice(c, Angle::Rear), s));
            }
        }
        let silver = render_png(choice(HeadsetColour::PlatinumSilver, Angle::Front), 1.0);
        let black = render_png(choice(HeadsetColour::Black, Angle::Front), 1.0);
        assert_ne!(silver, black);
        assert!(render_png(choice(HeadsetColour::Black, Angle::Front), 2.0).len() > black.len());
    }
}
