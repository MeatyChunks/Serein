//! Native component controls; transport authorization and validation stay in client-core.
use crate::{
	avatars::{Avatars, Surface},
	design, dialog, markdown,
};
use client_core::{Command, State};
use model::{Component, Id, Message};

pub(crate) type Action = (Id, String, Vec<String>);

pub(crate) struct MediaUi<'a> {
	pub component_viewing: &'a mut Option<(Id, u64)>,
	pub viewing: &'a mut Option<(Id, Id)>,
	pub download: &'a mut crate::attachments::DownloadUi,
	pub audio: &'a mut crate::audio::AudioUi,
	pub video: &'a mut crate::video::VideoUi,
	pub profile: &'a mut crate::profiles::ProfileSession,
	pub channel_reference: &'a mut Option<Id>,
}

#[derive(Default)]
pub(crate) struct Components {
	// One active select draft, bounded by the protocol's 25 values; edits replace its identity.
	generation: u64,
	channel: Option<Id>,
	revealed: std::collections::BTreeSet<u64>,
	text_revealed: std::collections::BTreeMap<u64, u32>,
	pub(crate) search: crate::member_search::Search,
	pub(crate) remote_search: bool,
	select: Option<(egui::Id, Vec<String>)>,
	modal: Option<(u64, Id, Vec<Component>)>,
	formatted: markdown::FormatCache,
	pub(crate) file_request: Option<String>,
	files: Vec<(String, Vec<(usize, String)>)>,
	pub(crate) pending_action: Option<Action>,
	// True while rendering a section accessory, so media draws as a compact thumbnail.
	accessory: bool,
}

impl Components {
	pub(crate) fn forget_message(&mut self, id: Id) {
		self.formatted.retain(|message| message != id);
		self.revealed.clear();
		self.text_revealed.clear();
		self.select = None;
		self.search = Default::default();
		self.remote_search = false;
	}
	pub(crate) fn modal_id(&self) -> Option<Id> {
		self.modal.as_ref().map(|(_, id, _)| *id)
	}
	pub(crate) fn set_files(&mut self, custom_id: &str, files: Vec<(usize, String)>) {
		if self.modal.is_none()
			|| custom_id.chars().count() > 100
			|| custom_id.len() > 400
			|| files.len() > 10
			|| files.iter().any(|(_, name)| name.len() > 1024)
		{
			return;
		}
		self.files.retain(|(key, _)| key != custom_id);
		if self.files.len() < 5 {
			self.files.push((custom_id.to_owned(), files));
		}
	}
	pub(crate) fn show(
		&mut self,
		ui: &mut egui::Ui,
		message: &Message,
		state: &State,
		avatars: &mut Avatars,
		opening: &mut Option<String>,
		media_ui: &mut MediaUi<'_>,
	) -> Option<Action> {
		if self.generation != state.generation || self.channel != state.selected {
			self.generation = state.generation;
			self.channel = state.selected;
			self.remote_search = false;
			self.revealed.clear();
			self.text_revealed.clear();
			self.select = None;
			self.search = Default::default();
			self.formatted = Default::default();
			self.files.clear();
			self.file_request = None;
		}
		let mut action = None;
		let identity = egui::Id::unique((
			state.generation,
			message.channel,
			message.id,
			&message.components,
		));
		let enabled = !message.forwarded
			&& !state.interactions.busy()
			&& state.gateway_connected
			&& state.freshness == model::Freshness::Fresh
			&& state.can_view(message.channel);
		ui.push_id(identity, |ui| {
			// Component trees keep Discord's message content width instead of the viewport.
			ui.set_max_width(ui.available_width().min(COMPONENTS_MAX_WIDTH));
			for (index, component) in message.components.iter().enumerate() {
				self.show_component(
					ui,
					component,
					identity.with(index),
					message,
					state,
					avatars,
					opening,
					enabled,
					&mut action,
					media_ui,
					design::MessageCardSurface::Conversation,
				);
			}
		});
		action
	}

	#[allow(clippy::too_many_arguments)]
	pub(crate) fn show_embed_components(
		&mut self,
		ui: &mut egui::Ui,
		embed_components: &[Component],
		embed_index: usize,
		message: &Message,
		state: &State,
		avatars: &mut Avatars,
		opening: &mut Option<String>,
		media_ui: &mut MediaUi<'_>,
		card_surface: design::MessageCardSurface,
	) {
		let identity = egui::Id::unique((
			state.generation,
			message.channel,
			message.id,
			("embed", embed_index),
		));
		let enabled = !message.forwarded
			&& !state.interactions.busy()
			&& state.gateway_connected
			&& state.freshness == model::Freshness::Fresh
			&& state.can_view(message.channel);
		let mut action = None;
		ui.push_id(identity, |ui| {
			ui.set_max_width(ui.available_width().min(COMPONENTS_MAX_WIDTH));
			for (index, component) in embed_components.iter().enumerate() {
				self.show_component(
					ui,
					component,
					identity.with(index),
					message,
					state,
					avatars,
					opening,
					enabled,
					&mut action,
					media_ui,
					card_surface,
				);
			}
		});
		if action.is_some() {
			self.pending_action = action;
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn show_component(
		&mut self,
		ui: &mut egui::Ui,
		c: &Component,
		id: egui::Id,
		message: &Message,
		state: &State,
		avatars: &mut Avatars,
		opening: &mut Option<String>,
		enabled: bool,
		action: &mut Option<Action>,
		media_ui: &mut MediaUi<'_>,
		card_surface: design::MessageCardSurface,
	) {
		let colors = design::palette(ui);
		// A container paints an opaque card even when its parent is translucent chat.
		let card_surface = if c.kind == 17 {
			design::MessageCardSurface::Opaque
		} else {
			card_surface
		};
		ui.push_id(id, |ui| {
			if c.spoiler {
				let revealed = self.revealed.contains(&id.value());
				if !revealed {
					let clicked = if self.accessory && c.kind == 11 {
						spoiler_tile(
							ui,
							egui::Vec2::splat(THUMBNAIL_SIZE),
							&crate::i18n::translate(
								"components-show-component-reveal-spoiler-component",
							),
						)
					} else {
						ui.button(crate::i18n::translate(
							"components-show-component-reveal-spoiler-component",
						))
						.clicked()
					};
					if clicked {
						if self.revealed.len() >= 256 {
							self.revealed.clear();
						}
						self.revealed.insert(id.value());
					}
					return;
				}
			}
			match c.kind {
				1 | 9 | 17 => {
					let frame = if c.kind == 17 {
						egui::Frame::new()
							.fill(design::opaque_card_fill(ui, colors.raised))
							.stroke(design::message_card_stroke(ui))
							.corner_radius(8)
							.inner_margin(egui::Margin {
								left: if c.accent_color.is_some() { 16 } else { 12 },
								right: 12,
								top: 12,
								bottom: 12,
							})
					} else {
						egui::Frame::new()
					};
					let shown = frame.show(ui, |ui| {
						if c.kind != 1 {
							// Discord separates stacked container/section children by 8px.
							ui.spacing_mut().item_spacing.y = 8.0;
						}
						if c.kind == 1 {
							ui.horizontal_wrapped(|ui| {
								for (index, child) in c.components.iter().enumerate() {
									self.show_component(
										ui,
										child,
										id.with(index),
										message,
										state,
										avatars,
										opening,
										enabled,
										action,
										media_ui,
										card_surface,
									);
								}
							});
						} else if c.kind == 9 {
							// The accessory takes its natural width at the right edge first;
							// the text column wraps in whatever remains.
							ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
								ui.spacing_mut().item_spacing.x = 16.0;
								if let Some(accessory) = &c.accessory {
									let thumbnail = accessory.kind == 11;
									let accessory_width = if thumbnail {
										THUMBNAIL_SIZE
									} else {
										(ui.available_width() * 0.45).max(40.0)
									};
									ui.scope(|ui| {
										ui.set_max_width(accessory_width);
										self.accessory = true;
										self.show_component(
											ui,
											accessory,
											id.with("accessory"),
											message,
											state,
											avatars,
											opening,
											enabled,
											action,
											media_ui,
											card_surface,
										);
										self.accessory = false;
									});
								}
								ui.allocate_ui_with_layout(
									egui::vec2(ui.available_width().max(40.0), 0.0),
									egui::Layout::top_down(egui::Align::Min),
									|ui| {
										ui.spacing_mut().item_spacing.y = 8.0;
										for (index, child) in c.components.iter().enumerate() {
											self.show_component(
												ui,
												child,
												id.with(index),
												message,
												state,
												avatars,
												opening,
												enabled,
												action,
												media_ui,
												card_surface,
											);
										}
									},
								);
							});
						} else {
							for (index, child) in c.components.iter().enumerate() {
								self.show_component(
									ui,
									child,
									id.with(index),
									message,
									state,
									avatars,
									opening,
									enabled,
									action,
									media_ui,
									card_surface,
								);
							}
						}
						if c.kind != 9
							&& let Some(accessory) = &c.accessory
						{
							self.show_component(
								ui,
								accessory,
								id.with("accessory"),
								message,
								state,
								avatars,
								opening,
								enabled,
								action,
								media_ui,
								card_surface,
							);
						}
					});
					if c.kind == 17
						&& let Some(rgb) = c.accent_color
					{
						let accent =
							egui::Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
						ui.painter().line_segment(
							[
								shown.response.rect.left_top() + egui::vec2(2.0, 6.0),
								shown.response.rect.left_bottom() - egui::vec2(-2.0, 6.0),
							],
							egui::Stroke::new(4.0, accent),
						);
					}
				}
				2 => {
					if c.style == Some(6) {
						return;
					}
					let label = component_label(c);
					let linked = c.style == Some(5);
					let text = egui::RichText::new(label);
					let text = if matches!(c.style, Some(1 | 3 | 4)) {
						text.color(colors.accent_text)
					} else {
						text
					};
					let image = c
						.emoji
						.as_ref()
						.and_then(|emoji| emoji.id)
						.and_then(|id| avatars.custom_image(ui.ctx(), id, 18.0, state.demo));
					let mut button = if let Some(image) = image {
						egui::Button::image_and_text(image, text)
							.image_tint_follows_text_color(false)
					} else {
						egui::Button::new(text)
					}
					.wrap_mode(if self.accessory {
						egui::TextWrapMode::Truncate
					} else {
						egui::TextWrapMode::Extend
					})
					.min_size(egui::vec2(0.0, 32.0))
					.corner_radius(6)
					.stroke(egui::Stroke::new(1.0, colors.border));
					if linked {
						button = button.right_text("   ");
					}
					button = match c.style {
						Some(1) => button.fill(colors.accent),
						Some(3) => button.fill(colors.positive),
						Some(4) => button.fill(colors.danger),
						_ => button,
					};
					let response = ui.add_enabled(!c.disabled && (linked || enabled), button);
					if linked {
						crate::icons::paint(
							ui.painter(),
							crate::icons::Icon::External,
							egui::Rect::from_center_size(
								response.rect.right_center() - egui::vec2(14.0, 0.0),
								egui::Vec2::splat(16.0),
							),
							colors.muted,
						);
					}
					if response.clicked() {
						if linked {
							*opening = c.url.as_deref().and_then(markdown::external_url);
						} else if let Some(custom_id) = &c.custom_id {
							*action = Some((message.id, custom_id.clone(), vec![]));
						}
					}
				}
				3 | 5..=8 => {
					let mut values = self
						.select
						.as_ref()
						.filter(|(key, _)| *key == id)
						.map(|(_, values)| values.clone())
						.unwrap_or_else(|| default_values(c));
					ui.add_enabled_ui(enabled && !c.disabled, |ui| {
						let changed = select(
							ui,
							c,
							&mut values,
							state,
							message.channel,
							&mut self.search.text,
							&mut self.remote_search,
							avatars,
						);
						let valid = values.len() >= usize::from(c.min_values.unwrap_or(1))
							&& values.len() <= usize::from(c.max_values.unwrap_or(1));
						if changed {
							self.select = Some((id, values.clone()));
						}
						let submit = if c.max_values.unwrap_or(1) == 1 {
							changed && valid
						} else {
							ui.add_enabled(
								valid,
								egui::Button::new(crate::i18n::translate(
									"components-show-component-submit-selection",
								)),
							)
							.clicked()
						};
						if submit && let Some(custom_id) = &c.custom_id {
							*action = Some((message.id, custom_id.clone(), values));
						}
					});
				}
				10 => {
					if self.text_revealed.len() >= 256
						&& !self.text_revealed.contains_key(&id.value())
					{
						self.text_revealed.clear();
					}
					let revealed = self.text_revealed.entry(id.value()).or_default();
					let mut surface = crate::select::Surface::new(ui, "component-text");
					let source = crate::mentions::MentionSource {
						state,
						channel: message.channel,
					};
					self.formatted
						.get_part(
							message.id,
							(id.value() % u64::from(u16::MAX)) as u16,
							c.content.as_deref().unwrap_or_default(),
						)
						.show_references(
							ui,
							opening,
							&message.mentions,
							Some(&source),
							media_ui.profile,
							(
								&state.channels,
								media_ui.channel_reference,
								&state.guilds,
								crate::mentions::known_roles(state, message.channel),
							),
							(avatars, state.demo, revealed),
							&mut surface,
							card_surface,
						);
					surface.finish(ui);
				}
				11 => {
					if let Some(media) = &c.media {
						// A section accessory renders as a compact square thumbnail without controls.
						let thumbnail = self.accessory;
						show_media(
							ui,
							media,
							c.description.as_deref().filter(|_| !thumbnail),
							avatars,
							state.demo,
							opening,
							message,
							media_ui,
							&mut self.revealed,
							thumbnail,
							false,
							card_surface,
						);
					}
				}
				12 => {
					if c.items.len() > 1
						&& c.items.iter().all(|item| {
							!item.media.is_video()
								&& find_attachment(&item.media, message)
									.is_none_or(|a| !crate::is_motion_video(&a.filename))
						}) {
						show_media_gallery(
							ui,
							&c.items,
							id,
							avatars,
							state.demo,
							message,
							media_ui,
							&mut self.revealed,
						);
					} else {
						for (index, item) in c.items.iter().enumerate() {
							let key = id.with(("gallery", index));
							let revealed = self.revealed.contains(&key.value());
							if item.spoiler && !revealed {
								if ui
									.button(crate::i18n::translate(
										"components-show-component-reveal-spoiler-media",
									))
									.clicked()
								{
									if self.revealed.len() >= 256 {
										self.revealed.clear();
									}
									self.revealed.insert(key.value());
								}
							} else {
								show_media(
									ui,
									&item.media,
									item.description.as_deref(),
									avatars,
									state.demo,
									opening,
									message,
									media_ui,
									&mut self.revealed,
									false,
									false,
									card_surface,
								);
							}
						}
					}
				}
				13 => {
					if let Some(file) = &c.file {
						show_media(
							ui,
							file,
							c.description.as_deref(),
							avatars,
							state.demo,
							opening,
							message,
							media_ui,
							&mut self.revealed,
							false,
							true,
							card_surface,
						);
					}
				}
				14 => {
					let pad = if c.spacing == Some(2) { 8.0 } else { 4.0 };
					ui.add_space(pad);
					if c.divider.unwrap_or(true) {
						ui.separator();
					}
					ui.add_space(pad);
				}
				_ => {
					ui.colored_label(
						colors.muted,
						format!(
							"{} ({})",
							crate::i18n::translate(
								"components-show-component-unsupported-component-type"
							),
							c.kind
						),
					);
				}
			}
		});
	}

	pub(crate) fn dialogs(
		&mut self,
		ctx: &egui::Context,
		state: &mut State,
		commands: &mut Vec<Command>,
		avatars: &mut Avatars,
		opening: &mut Option<String>,
	) {
		let Some(modal) = state.interactions.modal.as_ref() else {
			if self.modal.take().is_some() {
				self.remote_search = false;
				self.search = Default::default();
			}
			self.files.clear();
			self.file_request = None;
			return;
		};
		if self
			.modal
			.as_ref()
			.is_none_or(|(generation, id, _)| *generation != state.generation || *id != modal.id)
		{
			let mut components = modal.components.clone();
			initialize(&mut components);
			self.files.clear();
			self.text_revealed.clear();
			self.search = Default::default();
			self.remote_search = false;
			self.modal = Some((state.generation, modal.id, components));
		}
		let mut submit = false;
		let busy = state.interactions.busy();
		let components = &mut self.modal.as_mut().expect("modal initialized").2;
		let response = dialog::Dialog::new(
			("application-modal", state.generation, modal.id),
			&modal.title,
		)
		.width(520.0)
		.show(ctx, |dialog| {
			let mut valid = true;
			dialog.content(|ui| {
				ui.add_enabled_ui(!busy, |ui| {
					egui::ScrollArea::vertical()
						.max_height(440.0)
						.show(ui, |ui| {
							ui.spacing_mut().item_spacing.y = 10.0;
							for (index, component) in components.iter_mut().enumerate() {
								ui.push_id(index, |ui| {
									valid &= field(
										ui,
										component,
										state,
										&mut self.file_request,
										&self.files,
										&mut self.search.text,
										&mut self.remote_search,
										avatars,
										opening,
										&mut self.formatted,
										&mut self.text_revealed,
									);
								});
							}
						});
				});
				if let Some(error) = state.interactions.error {
					ui.colored_label(design::palette(ui).danger, error);
				}
				if busy {
					ui.label(crate::i18n::translate("components-dialogs-submitting"));
				}
			});
			dialog.footer(|ui| {
				ui.add_enabled_ui(valid && !busy, |ui| {
					submit =
						dialog::action(ui, "components-dialogs-submit", dialog::Action::Primary)
							.clicked();
				});
			});
		});
		let submitted = submit.then(|| components.clone());
		if response.close && !busy {
			state.dismiss_interaction_modal();
			self.modal = None;
		} else if let Some(submitted) = submitted
			&& let Some(command) = state.submit_interaction_modal(submitted)
		{
			commands.push(command);
		}
	}
}

fn component_label(c: &Component) -> String {
	let emoji = c
		.emoji
		.as_ref()
		.filter(|emoji| emoji.id.is_none())
		.and_then(|emoji| emoji.name.as_deref())
		.unwrap_or_default();
	let label = c.label.as_deref().unwrap_or("Button");
	if emoji.is_empty() {
		label.to_owned()
	} else {
		format!("{emoji} {label}")
	}
}
fn default_values(c: &Component) -> Vec<String> {
	if !c.values.is_empty() {
		c.values.clone()
	} else if !c.default_values.is_empty() {
		c.default_values
			.iter()
			.map(|value| value.id.to_string())
			.collect()
	} else {
		c.options
			.iter()
			.filter(|option| option.default)
			.map(|option| option.value.clone())
			.collect()
	}
}
fn initialize(components: &mut [Component]) {
	for c in components {
		if matches!(c.kind, 3 | 5..=8 | 19 | 21 | 22) {
			c.values = default_values(c);
			let limit = usize::from(c.max_values.unwrap_or(if c.kind == 22 {
				c.options.len() as u16
			} else {
				1
			}))
			.min(25);
			c.values.truncate(limit);
			let mut unique = std::collections::BTreeSet::new();
			c.values
				.retain(|value| value.len() <= 400 && unique.insert(value.clone()));
		}
		if c.kind == 23 {
			c.checked = Some(c.checked.unwrap_or(c.default));
		}
		if c.kind == 21
			&& let Some(value) = &c.value
		{
			c.values = vec![value.clone()];
		}
		initialize(&mut c.components);
		if let Some(child) = &mut c.component {
			initialize(std::slice::from_mut(child.as_mut()));
		}
	}
}
#[allow(clippy::too_many_arguments)]
fn select(
	ui: &mut egui::Ui,
	c: &Component,
	values: &mut Vec<String>,
	state: &State,
	channel: Id,
	query: &mut String,
	remote_search: &mut bool,
	avatars: &mut Avatars,
) -> bool {
	let mut changed = false;
	let colors = design::palette(ui);
	let users = if matches!(c.kind, 5 | 7) && !values.is_empty() {
		crate::mentions::known_users(state, channel)
	} else {
		Vec::new()
	};
	let selected_text = values
		.iter()
		.map(|value| {
			c.options
				.iter()
				.find(|option| option.value == *value)
				.map(|option| option.label.clone())
				.or_else(|| {
					value
						.parse::<Id>()
						.ok()
						.and_then(|id| state.channel(id))
						.map(|channel| channel.name.clone())
				})
				.or_else(|| {
					crate::mentions::known_roles(state, channel)
						.iter()
						.find(|role| Some(role.id) == value.parse::<Id>().ok())
						.map(|role| role.name.clone())
				})
				.or_else(|| {
					users
						.iter()
						.find(|user| Some(user.id) == value.parse::<Id>().ok())
						.map(|user| user.name.clone())
				})
				.unwrap_or_else(|| value.clone())
		})
		.collect::<Vec<_>>()
		.join(", ");
	let width = ui.available_width().min(400.0);
	egui::ComboBox::from_id_salt("selection")
		.width(width)
		.height(360.0)
		.selected_text(crate::i18n::translate_if_key(
			&(if selected_text.is_empty() {
				crate::i18n::translate_if_key(
					c.placeholder
						.as_deref()
						.unwrap_or("components-select-choose-options"),
				)
			} else {
				selected_text.clone()
			}),
		))
		.show_ui(ui, |ui| {
			ui.set_min_width((width - 16.0).max(40.0));
			if (c.min_values == Some(0) || !c.required)
				&& !values.is_empty()
				&& ui
					.button(crate::i18n::translate("components-select-clear-selection"))
					.clicked()
			{
				values.clear();
				changed = true;
				*remote_search = false;
				ui.close();
			}
			if (c.kind != 3 || c.options.len() > 10)
				&& ui
					.add(
						egui::TextEdit::singleline(query)
							.align(egui::Align2::LEFT_CENTER)
							.hint_text(crate::i18n::translate("components-select-search-options"))
							.char_limit(64),
					)
					.changed()
			{
				*remote_search = matches!(c.kind, 5 | 7);
			}
			let filter = if c.kind == 3 && c.options.len() <= 10 {
				String::new()
			} else {
				query.to_lowercase()
			};
			// Keep at most 100 matching labels (64 KiB); search still traverses the available catalog.
			let mut options = Vec::<(String, String)>::new();
			let mut bytes = 0;
			let mut add = |value: String, label: &str| {
				if options.len() < 100
					&& bytes + value.len() + label.len() <= 64 * 1024
					&& label.to_lowercase().contains(&filter)
					&& !options.iter().any(|(key, _)| *key == value)
				{
					bytes += value.len() + label.len();
					options.push((value, label.to_owned()));
				}
			};
			for option in &c.options {
				add(option.value.clone(), &option.label);
			}
			if matches!(c.kind, 5 | 7) {
				for user in crate::mentions::known_users(state, channel) {
					add(user.id.to_string(), &user.name);
				}
				for view in &state.member_search {
					if view
						.request
						.as_ref()
						.is_some_and(|request| request.channel == channel)
					{
						for member in &view.rows {
							add(member.user.id.to_string(), &member.user.name);
						}
					}
				}
			}
			if matches!(c.kind, 6 | 7) {
				for role in crate::mentions::known_roles(state, channel) {
					add(role.id.to_string(), &role.name);
				}
			}
			if c.kind == 8 {
				let guild = state.channel(channel).and_then(|item| item.guild);
				for item in state.channels.iter().filter(|item| {
					item.guild == guild
						&& state.can_view(item.id)
						&& (c.channel_types.is_empty() || c.channel_types.contains(&item.kind))
				}) {
					add(item.id.to_string(), &item.name);
				}
			}
			for (value, label) in &options {
				let selected = values.contains(value);
				let option = c.options.iter().find(|option| option.value == *value);
				let mut job = egui::text::LayoutJob::default();
				job.append(
					&format!(
						"{}{}{}",
						if selected { "✓  " } else { "" },
						option
							.and_then(|o| o.emoji.as_ref())
							.filter(|e| e.id.is_none())
							.and_then(|e| e.name.as_ref())
							.map(|name| format!("{name} "))
							.unwrap_or_default(),
						label
					),
					0.0,
					egui::TextFormat {
						font_id: egui::FontId::proportional(14.0),
						color: colors.text,
						..Default::default()
					},
				);
				if let Some(description) = option.and_then(|option| option.description.as_deref()) {
					job.append(
						&format!("\n{description}"),
						0.0,
						egui::TextFormat {
							font_id: egui::FontId::proportional(12.0),
							color: colors.muted,
							..Default::default()
						},
					);
				}
				let image = option
					.and_then(|option| option.emoji.as_ref())
					.and_then(|emoji| emoji.id)
					.and_then(|id| avatars.custom_image(ui.ctx(), id, 24.0, state.demo));
				let button = if let Some(image) = image {
					egui::Button::image_and_text(image, job).image_tint_follows_text_color(false)
				} else {
					egui::Button::new(job)
				};
				if ui
					.add_enabled(
						c.max_values.unwrap_or(1) == 1
							|| selected || values.len() < usize::from(c.max_values.unwrap_or(1)),
						button
							.selected(selected)
							.min_size(egui::vec2(ui.available_width(), 40.0)),
					)
					.clicked()
				{
					if c.max_values.unwrap_or(1) == 1 {
						*values = vec![value.clone()];
						ui.close();
					} else if selected {
						values.retain(|existing| existing != value);
					} else {
						values.push(value.clone());
					}
					changed = true;
					*remote_search = false;
				}
			}
			if options.is_empty() {
				ui.label(crate::i18n::translate(
					"components-select-no-matching-options-loaded",
				));
			}
			if options.len() == 100 {
				ui.small(crate::i18n::translate(
					"components-select-refine-your-search-to-see-more-results",
				));
			}
		});
	if matches!(c.kind, 5..=8) {
		ui.small(crate::i18n::translate(
			"components-select-type-to-search-members-available-roles-and-channels-are-listed",
		));
	}
	changed
}
#[allow(clippy::too_many_arguments)]
fn field(
	ui: &mut egui::Ui,
	c: &mut Component,
	state: &State,
	file_request: &mut Option<String>,
	files: &[(String, Vec<(usize, String)>)],
	query: &mut String,
	remote_search: &mut bool,
	avatars: &mut Avatars,
	opening: &mut Option<String>,
	formatted: &mut markdown::FormatCache,
	revealed: &mut std::collections::BTreeMap<u64, u32>,
) -> bool {
	let mut valid = true;
	ui.push_id((c.id, c.custom_id.clone()), |ui| {
		if let Some(label) = &c.label {
			ui.strong(label);
		}
		if let Some(description) = &c.description {
			ui.small(description);
		}
		match c.kind {
			1 | 18 => {
				for (index, child) in c.components.iter_mut().enumerate() {
					ui.push_id(index, |ui| {
						valid &= field(
							ui,
							child,
							state,
							file_request,
							files,
							query,
							remote_search,
							avatars,
							opening,
							formatted,
							revealed,
						);
					});
				}
				if let Some(child) = &mut c.component {
					valid &= field(
						ui,
						child,
						state,
						file_request,
						files,
						query,
						remote_search,
						avatars,
						opening,
						formatted,
						revealed,
					);
				}
			}
			4 => {
				let value = c.value.get_or_insert_with(String::new);
				let edit = if c.style == Some(2) {
					egui::TextEdit::multiline(value).desired_rows(5)
				} else {
					egui::TextEdit::singleline(value).align(egui::Align2::LEFT_CENTER)
				};
				ui.add(
					edit.char_limit(usize::from(c.max_length.unwrap_or(4000)).min(4000))
						.font(egui::FontId::proportional(15.0))
						.margin(egui::vec2(12.0, 10.0))
						.hint_text(c.placeholder.as_deref().unwrap_or_default())
						.desired_width(f32::INFINITY),
				);
				let length = value.chars().count();
				valid = (!c.required && length == 0)
					|| (length >= usize::from(c.min_length.unwrap_or(u16::from(c.required)))
						&& length <= usize::from(c.max_length.unwrap_or(4000)));
			}
			3 | 5..=8 => {
				let mut values = std::mem::take(&mut c.values);
				select(
					ui,
					c,
					&mut values,
					state,
					state.selected.unwrap_or(Id(0)),
					query,
					remote_search,
					avatars,
				);
				valid = (!c.required && values.is_empty())
					|| (values.len() >= usize::from(c.min_values.unwrap_or(1))
						&& values.len() <= usize::from(c.max_values.unwrap_or(1)));
				c.values = values;
			}
			21 | 22 => {
				for option in &c.options {
					let chosen = c.values.contains(&option.value);
					let clicked = if c.kind == 21 {
						ui.radio(chosen, &option.label).clicked()
					} else {
						let mut selected = chosen;
						ui.checkbox(&mut selected, &option.label).changed()
					};
					if clicked {
						if chosen {
							c.values.retain(|value| value != &option.value);
						} else if c.kind == 21 {
							c.values = vec![option.value.clone()];
						} else if c.values.len()
							< usize::from(c.max_values.unwrap_or(c.options.len() as u16))
						{
							c.values.push(option.value.clone());
						}
					}
					if let Some(description) = &option.description {
						ui.label(
							egui::RichText::new(description)
								.small()
								.color(design::palette(ui).muted),
						);
					}
				}
				if c.kind == 21 {
					c.value = c.values.first().cloned();
				}
				valid = (!c.required && c.values.is_empty())
					|| (c.values.len() >= usize::from(c.min_values.unwrap_or(1))
						&& c.values.len()
							<= usize::from(c.max_values.unwrap_or(if c.kind == 21 {
								1
							} else {
								c.options.len() as u16
							})));
			}
			23 => {
				ui.checkbox(
					c.checked.get_or_insert(false),
					c.label
						.clone()
						.unwrap_or_else(|| crate::i18n::translate("components-field-confirm")),
				);
				valid = true;
			}
			10 => {
				let key = ui.scope_id().value();
				if revealed.len() >= 256 && !revealed.contains_key(&key) {
					revealed.clear();
				}
				let mut surface = crate::select::Surface::new(ui, "modal-markdown");
				let source = crate::mentions::MentionSource {
					state,
					channel: state.selected.unwrap_or(Id(0)),
				};
				formatted
					.get(Id(key), c.content.as_deref().unwrap_or_default())
					.show_references(
						ui,
						opening,
						&[],
						Some(&source),
						&mut crate::profiles::ProfileSession::default(),
						(&state.channels, &mut None, &state.guilds, &[]),
						(avatars, state.demo, revealed.entry(key).or_default()),
						&mut surface,
						crate::design::MessageCardSurface::Opaque,
					);
				surface.finish(ui);
			}
			19 => {
				if !c.file_types.is_empty() {
					ui.small(format!(
						"{}: {}",
						crate::i18n::translate("components-field-allowed-files"),
						c.file_types.join(", ")
					));
				}
				if ui
					.button(crate::i18n::translate("components-field-choose-files"))
					.clicked()
				{
					*file_request = c.custom_id.clone();
				}
				if let Some((_, selected)) = files
					.iter()
					.find(|(key, _)| Some(key) == c.custom_id.as_ref())
				{
					c.values = selected
						.iter()
						.map(|(index, _)| index.to_string())
						.collect();
					for (_, name) in selected {
						ui.label(name);
					}
				}
				valid = (!c.required && c.values.is_empty())
					|| (c.values.len() >= usize::from(c.min_values.unwrap_or(1))
						&& c.values.len() <= usize::from(c.max_values.unwrap_or(1)));
			}
			_ => {
				ui.label(format!(
					"{} ({}).",
					crate::i18n::translate("components-field-unsupported-form-field-type"),
					c.kind
				));
				valid = false;
			}
		}
	});
	valid
}
/// Square side of a section thumbnail accessory, matching Discord's compact card art.
const THUMBNAIL_SIZE: f32 = 80.0;
/// Discord lays component trees out in the message content column, not across the viewport.
const COMPONENTS_MAX_WIDTH: f32 = 520.0;

fn find_attachment<'a>(
	media: &model::ComponentMedia,
	message: &'a Message,
) -> Option<&'a model::Attachment> {
	let url = resolve_media(&media.url, message);
	message.attachments.iter().find(|attachment| {
		media.url.strip_prefix("attachment://") == Some(attachment.filename.as_str())
			|| url.is_some_and(|url| {
				attachment.media.url.as_deref() == Some(url)
					|| attachment.media.proxy_url.as_deref() == Some(url)
			})
	})
}

fn spoiler_tile(ui: &mut egui::Ui, size: egui::Vec2, label: &str) -> bool {
	let colors = design::palette(ui);
	let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
	response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
	let painter = ui.painter().with_clip_rect(rect);
	painter.rect_filled(rect, 6, design::opaque_card_fill(ui, colors.base));
	painter.rect_stroke(
		rect,
		6,
		design::message_card_stroke(ui),
		egui::StrokeKind::Inside,
	);
	let pill_text = painter.layout_no_wrap(
		"SPOILER".to_owned(),
		egui::FontId::new(11.0, design::semibold_family(ui.ctx())),
		egui::Color32::WHITE,
	);
	let pill = egui::Rect::from_center_size(
		rect.center(),
		(pill_text.size() + egui::vec2(14.0, 8.0)).min(rect.size()),
	);
	painter.rect_filled(
		pill,
		12,
		if response.hovered() {
			egui::Color32::from_black_alpha(220)
		} else {
			egui::Color32::from_black_alpha(180)
		},
	);
	painter.galley(
		pill.center() - pill_text.size() * 0.5,
		pill_text,
		egui::Color32::WHITE,
	);
	response
		.on_hover_text(label)
		.on_hover_cursor(egui::CursorIcon::PointingHand)
		.clicked()
}

#[allow(clippy::too_many_arguments)]
fn show_media_gallery(
	ui: &mut egui::Ui,
	items: &[model::ComponentMediaItem],
	id: egui::Id,
	avatars: &mut Avatars,
	demo: bool,
	message: &Message,
	media_ui: &mut MediaUi<'_>,
	revealed: &mut std::collections::BTreeSet<u64>,
) {
	let width = ui
		.available_width()
		.clamp(1.0, crate::avatars::media::MEDIA_MAX_WIDTH);
	let height = crate::embeds::gallery_rect(items.len(), items.len() - 1, width).bottom();
	let (area, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
	for (index, item) in items.iter().enumerate() {
		let rect =
			crate::embeds::gallery_rect(items.len(), index, width).translate(area.min.to_vec2());
		let key = id.with(("gallery", index));
		let is_revealed = revealed.contains(&key.value());
		ui.scope_builder(
			egui::UiBuilder::new()
				.id_salt(("component-gallery", index))
				.max_rect(rect),
			|ui| {
				if item.spoiler && !is_revealed {
					let label =
						crate::i18n::translate("components-show-component-reveal-spoiler-media");
					if spoiler_tile(ui, rect.size(), &label) {
						if revealed.len() >= 256 {
							revealed.clear();
						}
						revealed.insert(key.value());
					}
					return;
				}
				let attachment = find_attachment(&item.media, message);
				let media = if let Some(att) = attachment {
					att.media.clone()
				} else {
					model::EmbedMedia {
						url: resolve_media(&item.media.url, message).map(str::to_owned),
						proxy_url: item.media.proxy_url.clone(),
						width: item.media.width.unwrap_or(640),
						height: item.media.height.unwrap_or(360),
						placeholder: Vec::new(),
					}
				};
				let painted = avatars
					.show_media(ui, &media, rect.size(), demo, Surface::Banner)
					.response;
				let response =
					ui.interact(painted.rect, painted.id.with("media"), egui::Sense::click());
				crate::attachments::embed_context_menu(&response, &media, media_ui.download, demo);
				let star =
					crate::embeds::gif_for_media(&media, media.url.as_deref(), false).map(|gif| {
						let favorite = media_ui.download.gif_favorites.contains(&gif.url);
						let star = crate::embeds::favorite_star(
							ui,
							&response,
							favorite,
							avatars.gifs_playing(ui.ctx()),
						);
						if star.clicked() {
							media_ui.download.gif_favorite_request = Some(gif);
						}
						star
					});
				let hover = item.description.as_deref().unwrap_or("Open image");
				response.widget_info(|| {
					egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), hover)
				});
				if response.has_focus() {
					ui.painter().rect_stroke(
						rect,
						5,
						ui.visuals().selection.stroke,
						egui::StrokeKind::Inside,
					);
				}
				if !star
					.as_ref()
					.is_some_and(|star| star.hovered() || star.clicked())
					&& response
						.on_hover_text(hover)
						.on_hover_cursor(egui::CursorIcon::PointingHand)
						.clicked()
				{
					if let Some(att) = attachment {
						*media_ui.viewing = Some((message.id, att.id));
						*media_ui.component_viewing = Some((
							message.id,
							egui::Id::unique((&message.components, &message.attachments)).value(),
						));
					} else {
						media_ui.download.view_embed(message.id, &media);
					}
				}
			},
		);
	}
}

fn show_file_card(
	ui: &mut egui::Ui,
	media: &model::ComponentMedia,
	description: Option<&str>,
	opening: &mut Option<String>,
	message: &Message,
	card_surface: design::MessageCardSurface,
) {
	let colors = design::palette(ui);
	let url = resolve_media(&media.url, message);
	let path = url.unwrap_or(&media.url);
	let clean = path.split(['?', '#']).next().unwrap_or(path);
	let filename = clean
		.rsplit_once('/')
		.map(|(_, name)| name)
		.filter(|name| !name.is_empty())
		.unwrap_or("attachment");
	let safe_url = url.and_then(markdown::external_url);
	egui::Frame::new()
		.fill(card_surface.fill(ui, colors.base))
		.stroke(design::message_card_stroke(ui))
		.corner_radius(8)
		.inner_margin(egui::Margin::symmetric(12, 10))
		.show(ui, |ui| {
			ui.set_max_width(ui.available_width().min(420.0));
			ui.horizontal(|ui| {
				crate::icons::inline(ui, crate::icons::Icon::FileText, 24.0, colors.muted);
				ui.vertical(|ui| {
					if let Some(target) = &safe_url {
						if ui
							.add(
								egui::Label::new(
									egui::RichText::new(filename).strong().color(colors.link),
								)
								.truncate()
								.sense(egui::Sense::click()),
							)
							.on_hover_cursor(egui::CursorIcon::PointingHand)
							.on_hover_text(target)
							.clicked()
						{
							*opening = Some(target.clone());
						}
					} else {
						ui.add(
							egui::Label::new(
								egui::RichText::new(filename)
									.strong()
									.color(colors.text_strong),
							)
							.truncate(),
						);
					}
					if let Some(description) = description.filter(|d| !d.is_empty()) {
						ui.small(description);
					}
				});
			});
		});
}

#[allow(clippy::too_many_arguments)]
fn show_media(
	ui: &mut egui::Ui,
	media: &model::ComponentMedia,
	description: Option<&str>,
	avatars: &mut Avatars,
	demo: bool,
	opening: &mut Option<String>,
	message: &Message,
	media_ui: &mut MediaUi<'_>,
	revealed: &mut std::collections::BTreeSet<u64>,
	thumbnail: bool,
	is_file: bool,
	card_surface: design::MessageCardSurface,
) {
	if let Some(attachment) = find_attachment(media, message) {
		let reveal_id = egui::Id::unique((
			"component-file-spoiler",
			message.id,
			&message.components,
			attachment.id,
		))
		.value();
		if attachment.spoiler && !revealed.contains(&reveal_id) {
			if ui
				.button(crate::i18n::translate(
					"components-show-media-reveal-spoiler-attachment",
				))
				.clicked()
			{
				if revealed.len() >= 256 {
					revealed.clear();
				}
				revealed.insert(reveal_id);
			}
			return;
		}
		if thumbnail {
			let painted = avatars
				.show_media(
					ui,
					&attachment.media,
					egui::vec2(THUMBNAIL_SIZE, THUMBNAIL_SIZE),
					demo,
					Surface::Banner,
				)
				.response;
			let response =
				ui.interact(painted.rect, painted.id.with("thumb"), egui::Sense::click());
			crate::attachments::media_context_menu(
				&response,
				attachment,
				media_ui.download,
				opening,
				demo,
			);
			if response
				.on_hover_cursor(egui::CursorIcon::PointingHand)
				.clicked()
			{
				*media_ui.viewing = Some((message.id, attachment.id));
				*media_ui.component_viewing = Some((
					message.id,
					egui::Id::unique((&message.components, &message.attachments)).value(),
				));
			}
			return;
		}
		let previous_view = *media_ui.viewing;
		let mut surface = crate::select::Surface::new(ui, ("component-attachment", attachment.id));
		crate::attachments::show_subset(
			ui,
			message,
			std::slice::from_ref(attachment),
			avatars,
			media_ui.viewing,
			opening,
			media_ui.download,
			media_ui.audio,
			media_ui.video,
			demo,
			&mut surface,
			card_surface,
		);
		surface.finish(ui);
		if *media_ui.viewing != previous_view
			|| media_ui
				.video
				.active
				.as_ref()
				.is_some_and(|(_, id, active)| *id == message.id && active == attachment)
		{
			*media_ui.component_viewing = Some((
				message.id,
				egui::Id::unique((&message.components, &message.attachments)).value(),
			));
		}
		if let Some(description) = description {
			ui.small(description);
		}
		return;
	}
	if is_file {
		show_file_card(ui, media, description, opening, message, card_surface);
		return;
	}
	let url = resolve_media(&media.url, message);
	let video_proxy = media
		.proxy_url
		.as_deref()
		.or(url)
		.filter(|candidate| media.is_video() && crate::embeds::is_proxied_video_url(candidate));
	if !thumbnail && let Some(proxy_str) = video_proxy {
		let path = proxy_str.split(['?', '#']).next().unwrap_or(proxy_str);
		let filename = path
			.rsplit_once('/')
			.map(|(_, name)| name)
			.filter(|name| !name.is_empty())
			.unwrap_or("video.mp4");
		let original = url
			.and_then(markdown::external_url)
			.unwrap_or_else(|| proxy_str.to_owned());
		let synthetic = model::Attachment {
			id: model::Id(
				egui::Id::unique((message.id, "component-video", proxy_str))
					.value()
					.max(1),
			),
			filename: filename.to_owned(),
			description: description.map(str::to_owned),
			content_type: media
				.content_type
				.clone()
				.or_else(|| Some("video/mp4".into())),
			size: 0,
			media: model::EmbedMedia {
				url: Some(original.clone()),
				proxy_url: Some(proxy_str.to_owned()),
				width: media.width.unwrap_or(480),
				height: media.height.unwrap_or(270),
				placeholder: Vec::new(),
			},
			spoiler: false,
			duration_ms: None,
			waveform: Vec::new(),
		};
		media_ui.video.show_embedded(
			ui,
			message,
			&synthetic,
			None,
			Some(original.as_str()),
			avatars,
			media_ui.download,
			opening,
			demo,
		);
		if let Some(description) = description {
			ui.small(description);
		}
		return;
	}
	let image = model::EmbedMedia {
		url: url.map(str::to_owned),
		proxy_url: media.proxy_url.clone(),
		width: media.width.unwrap_or(320),
		height: media.height.unwrap_or(180),
		..Default::default()
	};
	if thumbnail {
		let painted = avatars
			.show_media(
				ui,
				&image,
				egui::vec2(THUMBNAIL_SIZE, THUMBNAIL_SIZE),
				demo,
				Surface::Banner,
			)
			.response;
		let response = ui.interact(painted.rect, painted.id.with("thumb"), egui::Sense::click());
		crate::attachments::embed_context_menu(&response, &image, media_ui.download, demo);
		if response
			.on_hover_cursor(egui::CursorIcon::PointingHand)
			.clicked()
		{
			media_ui.download.view_embed(message.id, &image);
		}
		return;
	}
	let painted = avatars
		.show_media(
			ui,
			&image,
			egui::vec2(
				ui.available_width()
					.min(crate::avatars::media::MEDIA_MAX_WIDTH),
				crate::avatars::media::MEDIA_MAX_HEIGHT,
			),
			demo,
			Surface::Inline,
		)
		.response;
	let response = ui.interact(painted.rect, painted.id.with("media"), egui::Sense::click());
	let hover = description.unwrap_or("Open image");
	response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), hover));
	crate::attachments::embed_context_menu(&response, &image, media_ui.download, demo);
	let star = crate::embeds::gif_for_media(&image, image.url.as_deref(), false).map(|gif| {
		let favorite = media_ui.download.gif_favorites.contains(&gif.url);
		let star =
			crate::embeds::favorite_star(ui, &response, favorite, avatars.gifs_playing(ui.ctx()));
		if star.clicked() {
			media_ui.download.gif_favorite_request = Some(gif);
		}
		star
	});
	if !star
		.as_ref()
		.is_some_and(|star| star.hovered() || star.clicked())
		&& response
			.on_hover_text(hover)
			.on_hover_cursor(egui::CursorIcon::PointingHand)
			.clicked()
	{
		media_ui.download.view_embed(message.id, &image);
	}
	if let Some(description) = description {
		ui.small(description);
	}
}

fn resolve_media<'a>(url: &'a str, message: &'a Message) -> Option<&'a str> {
	if let Some(filename) = url.strip_prefix("attachment://") {
		message
			.attachments
			.iter()
			.find(|attachment| attachment.filename == filename)
			.and_then(|attachment| attachment.media.url.as_deref())
	} else {
		Some(url)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn modal_defaults_survive_nested_labels_without_replacing_explicit_values() {
		let mut components = vec![Component {
			kind: 18,
			component: Some(Box::new(Component {
				kind: 23,
				default: true,
				..Default::default()
			})),
			components: vec![Component {
				kind: 21,
				value: Some("chosen".into()),
				..Default::default()
			}],
			..Default::default()
		}];
		initialize(&mut components);
		assert_eq!(
			components[0].component.as_ref().unwrap().checked,
			Some(true)
		);
		assert_eq!(components[0].components[0].values, ["chosen"]);
		components[0].component.as_mut().unwrap().checked = Some(false);
		initialize(&mut components);
		assert_eq!(
			components[0].component.as_ref().unwrap().checked,
			Some(false)
		);
	}

	#[test]
	fn layout_view_container_gallery_and_text_references_work_like_discord() {
		let mut state = test_support::demo_state();
		let channel = state.selected.expect("selected channel");
		let target_channel = state
			.channels
			.iter()
			.find(|c| c.id != channel && state.can_view(c.id))
			.map(|c| c.id)
			.unwrap_or(channel);
		let mut message = test_support::message(1, Id(42));
		message.channel = channel;
		message.embeds = vec![model::Embed {
			kind: "layoutview".into(),
			components: vec![Component {
				kind: 17,
				accent_color: Some(0x5865F2),
				components: vec![
					Component {
						kind: 9,
						components: vec![Component {
							kind: 10,
							content: Some(format!(
								"### Layout Heading\nJump to <#{target_channel}>"
							)),
							..Default::default()
						}],
						accessory: Some(Box::new(Component {
							kind: 11,
							media: Some(model::ComponentMedia {
								url: "https://cdn.discordapp.com/attachments/1/2/thumb.png".into(),
								width: Some(160),
								height: Some(160),
								..Default::default()
							}),
							..Default::default()
						})),
						..Default::default()
					},
					Component {
						kind: 12,
						items: vec![
							model::ComponentMediaItem {
								media: model::ComponentMedia {
									url: "https://cdn.discordapp.com/attachments/1/2/first.png"
										.into(),
									width: Some(640),
									height: Some(360),
									..Default::default()
								},
								description: Some("First tile".into()),
								spoiler: false,
							},
							model::ComponentMediaItem {
								media: model::ComponentMedia {
									url: "https://cdn.discordapp.com/attachments/1/2/second.png"
										.into(),
									width: Some(640),
									height: Some(360),
									..Default::default()
								},
								description: Some("Second tile".into()),
								spoiler: false,
							},
						],
						..Default::default()
					},
					Component {
						kind: 1,
						components: vec![Component {
							kind: 2,
							style: Some(1),
							label: Some("Confirm".into()),
							custom_id: Some("layout_confirm".into()),
							..Default::default()
						}],
						..Default::default()
					},
				],
				..Default::default()
			}],
			..Default::default()
		}];
		state.gateway_connected = true;
		state.freshness = model::Freshness::Fresh;
		let ctx = egui::Context::default();
		crate::design::apply(&ctx);
		let mut cache = markdown::FormatCache::default();
		let mut avatars = Avatars::default();
		let mut opening = None;
		let mut components = Components::default();
		let mut download = crate::attachments::DownloadUi::default();
		let mut audio = crate::audio::AudioUi::default();
		let mut video = crate::video::VideoUi::default();
		let mut profile = crate::profiles::ProfileSession::default();
		let mut channel_ref = None;
		let mut comp_viewing = None;
		let mut viewing = None;
		ctx.run_ui(
			egui::RawInput {
				screen_rect: Some(egui::Rect::from_min_size(
					egui::Pos2::ZERO,
					egui::vec2(640.0, 720.0),
				)),
				..Default::default()
			},
			|ui| {
				crate::embeds::show(
					ui,
					&message,
					&mut cache,
					&mut avatars,
					&mut opening,
					&state,
					design::MessageCardSurface::Conversation,
					false,
					&mut components,
					&mut MediaUi {
						component_viewing: &mut comp_viewing,
						viewing: &mut viewing,
						download: &mut download,
						audio: &mut audio,
						video: &mut video,
						profile: &mut profile,
						channel_reference: &mut channel_ref,
					},
				);
			},
		)
		.drop_without_applying_deltas();
		assert!(
			crate::embeds::estimated_height(&message.embeds) > 200.0,
			"LayoutView embeds contribute component height to virtual scroll estimation"
		);
	}
}
