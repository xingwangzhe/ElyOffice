use std::{io::Cursor, net::TcpListener};

use gpui::{ImageFormat, TestAppContext};

use super::*;
use crate::tests::settle;

fn png() -> Vec<u8> {
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    png.into_inner()
}

fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(ely_gpui_component::init_for_tests);
    cx.update(|cx| {
        ely_ui::settings::install_defaults(cx);
        ely_ui::i18n::install_as("en-US", cx);
    });
}

#[test]
fn a_loaded_picture_becomes_a_real_png_and_bad_data_fails() {
    let picture = HeldPicture::new(ImageFormat::Png, png());
    let copied = picture::png(&picture).unwrap();
    let decoded = image::load_from_memory(&copied).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (3, 2));
    assert_eq!(decoded.get_pixel(0, 0).0, [10, 20, 30, 255]);
    let bad = HeldPicture::new(ImageFormat::Png, b"broken".to_vec());
    assert!(picture::png(&bad).is_err());
}

#[test]
fn saving_a_loaded_cid_picture_writes_the_held_bytes() {
    let mut cx = TestAppContext::single();
    init(&mut cx);
    let bytes = png();
    let pictures = Rc::new(Pictures::from([(
        "ely-inline:0".into(),
        Ok(HeldPicture::new(ImageFormat::Png, bytes.clone())),
    )]));
    let (reader, cx) = cx.add_window_view(|_, cx| BodyReader::new(Rc::from([]), pictures, cx));
    cx.update(|_, app| reader.update(app, |reader, cx| reader.save_picture("ely-inline:0", cx)));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved.png");
    let chosen = path.clone();
    cx.run_until_parked();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    settle(cx, |_| path.exists());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn blocked_picture_actions_never_fetch() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/blocked.png", listener.local_addr().unwrap());
    let mut cx = TestAppContext::single();
    init(&mut cx);
    let (reader, cx) =
        cx.add_window_view(|_, cx| BodyReader::new(Rc::from([]), Rc::new(Pictures::new()), cx));
    cx.update(|_, app| {
        reader.update(app, |reader, cx| {
            reader.copy_picture(&url, cx);
            reader.save_picture(&url, cx);
            assert!(reader.said.as_ref().unwrap().1.contains("Not downloaded"));
        })
    });
    cx.run_until_parked();
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn a_body_image_round_trips_through_the_system_clipboard() {
    if std::env::var_os("ELY_CLIPBOARD_TEST").is_none() {
        return eprintln!("SKIP: ELY_CLIPBOARD_TEST=1 replaces the clipboard");
    }
    let mut cx = TestAppContext::single();
    init(&mut cx);
    let pictures = Rc::new(Pictures::from([(
        "ely-inline:0".into(),
        Ok(HeldPicture::new(ImageFormat::Png, png())),
    )]));
    let (reader, cx) = cx.add_window_view(|_, cx| BodyReader::new(Rc::from([]), pictures, cx));
    cx.update(|_, app| reader.update(app, |reader, cx| reader.copy_picture("ely-inline:0", cx)));
    settle(cx, |app| reader.read(app).said.is_some());
    let Some(Clip::Png(bytes)) = clipboard::read().unwrap() else {
        panic!("the clipboard holds image data")
    };
    let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (3, 2));
    assert_eq!(image.get_pixel(0, 0).0, [10, 20, 30, 255]);
}

#[test]
fn body_shortcuts_leave_a_focused_search_field_alone() {
    use ely_gpui_component::forms::{Input, TextInput};
    use gpui::{Focusable as _, Render, Window};
    struct Page {
        reader: Entity<BodyReader>,
        input: Entity<TextInput>,
    }
    impl Render for Page {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .child(Input::new(&self.input))
                .child(self.reader.clone())
        }
    }
    let mut cx = TestAppContext::single();
    init(&mut cx);
    let (page, cx) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| {
            let mut input = TextInput::new(window, cx).label("Search");
            input.set_text("search words", cx);
            input
        });
        let blocks = vec![MarkdownBlock::Paragraph(
            ely_gpui_component::documents::Inline {
                text: "body 中文".into(),
                ..Default::default()
            },
        )]
        .into();
        let reader = cx.new(|cx| BodyReader::new(blocks, Rc::new(Pictures::new()), cx));
        Page { reader, input }
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let focus = page.read(app).input.read(app).focus_handle(app);
        window.focus(&focus, app);
    });
    cx.simulate_keystrokes("secondary-a secondary-c");
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().as_deref(),
        Some("search words")
    );
    cx.update(|window, app| {
        let focus = page
            .read(app)
            .reader
            .read(app)
            .selection
            .read(app)
            .focus_handle(app);
        window.focus(&focus, app);
    });
    cx.simulate_keystrokes("secondary-a secondary-c");
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().as_deref(),
        Some("body 中文")
    );
}
