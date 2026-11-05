//! Read-only body actions, for mail and message files.

use std::rc::Rc;

use ely_gpui_component::{
    documents::{
        HeldPicture, MarkdownBlock, MarkdownContext, MarkdownRenderer, MarkdownSelection, Pictures,
    },
    feedback::Alert,
    menus::{ContextMenu, Menu, MenuItem},
    primitives::Severity,
};
use ely_platform::clipboard::{self, Clip};
use ely_ui::menu::Offer;
use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div,
};

use crate::mail::text;

mod picture;
#[cfg(test)]
mod tests;

pub struct BodyReader {
    pub blocks: Rc<[MarkdownBlock]>,
    pub pictures: Rc<Pictures>,
    pub selection: Entity<MarkdownSelection>,
    asked: Option<Offer<MarkdownContext>>,
    said: Option<(Severity, SharedString)>,
}

impl BodyReader {
    pub fn new(
        blocks: Rc<[MarkdownBlock]>,
        pictures: Rc<Pictures>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            blocks,
            pictures,
            selection: cx.new(MarkdownSelection::new),
            asked: None,
            said: None,
        }
    }

    fn picture(&self, url: &str, cx: &gpui::App) -> Result<HeldPicture, SharedString> {
        self.pictures
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(text(cx, "mail.picture.waiting", &[])))
    }

    fn ask(&mut self, target: MarkdownContext, cx: &mut Context<Self>) {
        self.asked = Some(Offer::new(
            self.asked.as_ref(),
            0,
            target.clone(),
            target.position,
        ));
        cx.notify();
    }

    fn menu(&self, target: &MarkdownContext, cx: &Context<Self>) -> Menu {
        let selection = self.selection.clone();
        let copy = MenuItem::new(text(cx, "mail.body.copy", &[]))
            .disabled(selection.read(cx).selected_text().is_empty())
            .on_click(move |_, cx| selection.update(cx, |selection, cx| selection.copy(cx)));
        let selection = self.selection.clone();
        let all = MenuItem::new(text(cx, "mail.body.all", &[]))
            .on_click(move |_, cx| selection.update(cx, |selection, cx| selection.select_all(cx)));
        let mut menu = Menu::new().item(copy).item(all);
        if let Some(url) = &target.image {
            let picture = self.picture(url, cx);
            let this = cx.weak_entity();
            let url_copy = url.clone();
            let copy = MenuItem::new(text(cx, "mail.body.copy_image", &[]))
                .disabled(picture.is_err())
                .on_click(move |_, cx| {
                    this.update(cx, |reader, cx| reader.copy_picture(&url_copy, cx))
                        .ok();
                });
            let this = cx.weak_entity();
            let url_save = url.clone();
            let save = MenuItem::new(text(cx, "mail.body.save_image", &[]))
                .disabled(picture.is_err())
                .on_click(move |_, cx| {
                    this.update(cx, |reader, cx| reader.save_picture(&url_save, cx))
                        .ok();
                });
            menu = menu.separator().item(copy).item(save);
            if let Err(why) = picture {
                menu = menu.item(MenuItem::new(why).disabled(true));
            }
        }
        if let Some(link) = &target.link {
            let link = link.clone();
            menu = menu.separator().item(
                MenuItem::new(text(cx, "mail.body.copy_link", &[])).on_click(move |_, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.clone()))
                }),
            );
        }
        menu
    }

    fn failed(&mut self, key: &str, error: &str, cx: &mut Context<Self>) {
        log::warn!("mail: a body picture action failed");
        self.said = Some((Severity::Danger, text(cx, key, &[("error", error)])));
        cx.notify();
    }

    fn copy_picture(&mut self, url: &str, cx: &mut Context<Self>) {
        let picture = match self.picture(url, cx) {
            Ok(picture) => picture,
            Err(error) => return self.failed("mail.body.copy_failed", &error, cx),
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let png = picture::png(&picture)?;
                    clipboard::write(&Clip::Png(png)).map_err(|error| error.to_string())
                })
                .await;
            this.update(cx, |reader, cx| match result {
                Ok(()) => {
                    log::info!("mail: a body picture was copied");
                    reader.said = Some((Severity::Success, text(cx, "mail.body.copied", &[])));
                    cx.notify();
                }
                Err(error) => reader.failed("mail.body.copy_failed", &error, cx),
            })
            .ok();
        })
        .detach();
    }

    fn save_picture(&mut self, url: &str, cx: &mut Context<Self>) {
        let picture = match self.picture(url, cx) {
            Ok(picture) => picture,
            Err(error) => return self.failed("mail.file.unsaved", &error, cx),
        };
        let Some(dirs) = directories::UserDirs::new() else {
            return self.failed(
                "mail.file.unsaved",
                &ely_ui::i18n::text(cx, "error.no_home", &[]),
                cx,
            );
        };
        let dir = dirs.download_dir().unwrap_or(dirs.home_dir());
        let name = format!("image.{}", picture.format().extension());
        let chosen = cx.prompt_for_new_path(dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => return,
                Ok(Err(error)) => {
                    this.update(cx, |reader, cx| {
                        reader.failed("mail.file.unsaved", &error.to_string(), cx)
                    })
                    .ok();
                    return;
                }
                Err(error) => {
                    this.update(cx, |reader, cx| {
                        reader.failed("mail.file.unsaved", &error.to_string(), cx)
                    })
                    .ok();
                    return;
                }
            };
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let result = cx
                .background_spawn(async move { ely_files::export(&path, None, picture.bytes(), 0) })
                .await;
            this.update(cx, |reader, cx| match result {
                Ok(_) => {
                    log::info!("mail: a body picture was saved");
                    reader.said = Some((
                        Severity::Success,
                        text(cx, "mail.file.saved", &[("name", &name)]),
                    ));
                    cx.notify();
                }
                Err(error) => reader.failed("mail.file.unsaved", &error.to_string(), cx),
            })
            .ok();
        })
        .detach();
    }
}

impl Render for BodyReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let drawn = MarkdownRenderer::blocks("mail-body", self.blocks.clone())
            .pictures(self.pictures.clone())
            .selection(&self.selection)
            .on_link(|url, _, cx| cx.open_url(url))
            .on_context(move |target, _, cx| {
                this.update(cx, |reader, cx| reader.ask(target, cx)).ok();
            });
        let menu = match &self.asked {
            Some(asked) => asked.host("mail-body-menu", self.menu(&asked.what, cx)),
            None => ContextMenu::new("mail-body-menu", Menu::new()).manual(None),
        };
        let said = self
            .said
            .clone()
            .map(|(severity, words)| Alert::new("mail-body-result", severity, words));
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(menu.child(drawn))
            .children(said)
    }
}
