//! The one provider player webview, created on demand over the theater stage the UI reserves.
use eframe::egui;
use std::sync::Arc;

#[derive(Default)]
pub struct WebPlayer {
	view: Option<(ui::WebVideo, platform::web_player::WebPlayer)>,
}
impl WebPlayer {
	pub fn close(&mut self) {
		self.view = None;
	}
	pub fn sync(
		&mut self,
		player: &mut ui::VideoUi,
		window: &Arc<winit::window::Window>,
		ctx: &egui::Context,
		allowed: bool,
		offline: bool,
	) {
		if !allowed {
			player.close_web();
		}
		let Some((_, _, video)) = player.web.as_ref() else {
			self.close();
			return;
		};
		if self.view.as_ref().is_none_or(|(open, _)| open != video) {
			self.close();
			if offline {
				player.web_notice =
					Some("Provider playback is disabled in the offline demo".into());
				return;
			}
			let wake = ctx.clone();
			match platform::web_player::WebPlayer::open(
				window.clone(),
				&video.embed_url,
				move || wake.request_repaint(),
			) {
				Ok(view) => self.view = Some((video.clone(), view)),
				Err(err) => {
					player.web_notice = Some(err.into());
					player.web_external = video.original_url.clone();
					player.close_web();
					return;
				}
			}
		}
		let Some((_, view)) = &self.view else {
			return;
		};
		if let Some(url) = view.external() {
			player.web_external = Some(url);
		}
		let scale = ctx.pixels_per_point();
		match player.web_bounds {
			Some(bounds) => view.set_bounds(
				(bounds.left() * scale).round() as i32,
				(bounds.top() * scale).round() as i32,
				(bounds.width() * scale).round() as u32,
				(bounds.height() * scale).round() as u32,
			),
			None => view.set_bounds(0, 0, 0, 0),
		}
	}
}
