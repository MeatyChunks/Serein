use serde::Serialize;
use serein_extension_sdk::{
	AppInvocation, AppOutput, AttachmentSnapshot, Element, EmbedSummarySnapshot, HostEffect,
	MessageDetailSnapshot, MessageReferenceSnapshot, MessageSnapshot, MessageStickerSnapshot,
	Output, PollAvailability, ReactionSnapshot, RichMessageSnapshot, UserSnapshot, serde_json,
};

const MAX_CLIPBOARD_BYTES: usize = 6 * 1024;
const MAX_PANEL_JSON_BYTES: usize = 16 * 1024;

#[derive(Serialize)]
struct RawMessageView<'a> {
	#[serde(skip_serializing_if = "Option::is_none")]
	id: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	channel_id: Option<&'a str>,
	#[serde(rename = "type", skip_serializing_if = "Option::is_none")]
	kind: Option<u8>,
	#[serde(skip_serializing_if = "Option::is_none")]
	author: Option<&'a UserSnapshot>,
	content: &'a str,
	#[serde(skip_serializing_if = "Option::is_none")]
	display_text: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	edited: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	attachment_count: Option<u16>,
	#[serde(skip_serializing_if = "Option::is_none")]
	reply_to: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	reference: Option<&'a MessageReferenceSnapshot>,
	#[serde(skip_serializing_if = "Option::is_none")]
	mention_everyone: Option<bool>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	mentions: Vec<&'a str>,
	#[serde(skip_serializing_if = "is_false")]
	mentions_truncated: bool,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	attachments: Vec<&'a AttachmentSnapshot>,
	#[serde(skip_serializing_if = "is_false")]
	attachments_truncated: bool,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	embeds: Vec<&'a EmbedSummarySnapshot>,
	#[serde(skip_serializing_if = "is_false")]
	embeds_truncated: bool,
	#[serde(skip_serializing_if = "is_false")]
	embeds_suppressed: bool,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	stickers: Vec<&'a MessageStickerSnapshot>,
	#[serde(skip_serializing_if = "is_false")]
	stickers_truncated: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	reactions: Option<&'a [ReactionSnapshot]>,
	#[serde(skip_serializing_if = "is_false")]
	reactions_truncated: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	poll: Option<PollAvailability>,
}

fn is_false(value: &bool) -> bool {
	!*value
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
	if text.len() <= max_bytes {
		return text.to_owned();
	}
	let mut end = max_bytes.saturating_sub(32);
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	format!("{}\n… (truncated)", &text[..end])
}

fn parse_option_id(raw: &str) -> &str {
	raw.split_once(" · ").map_or(raw, |(id, _)| id).trim()
}

fn option_label(message: &MessageSnapshot) -> String {
	let preview: String = message
		.content
		.lines()
		.next()
		.unwrap_or("")
		.chars()
		.take(36)
		.collect();
	if preview.is_empty() {
		format!("{} · {} (no text)", message.id, message.author.name)
	} else {
		format!("{} · {}: {}", message.id, message.author.name, preview)
	}
}

fn resolve_message<'a>(
	messages: &'a [MessageSnapshot],
	requested_id: Option<&str>,
	selected_text: Option<&str>,
) -> Option<&'a MessageSnapshot> {
	if let Some(id) = requested_id
		.map(parse_option_id)
		.filter(|id| !id.is_empty())
		&& let Some(found) = messages.iter().find(|m| m.id == id)
	{
		return Some(found);
	}
	if let Some(selected) = selected_text {
		if let Some(found) = messages.iter().rev().find(|m| m.content == selected) {
			return Some(found);
		}
		if let Some(found) = messages
			.iter()
			.rev()
			.find(|m| !m.content.is_empty() && selected.ends_with(&m.content))
		{
			return Some(found);
		}
	}
	messages.last()
}

fn handle(input: AppInvocation) -> AppOutput {
	if input.app_event.is_some() || input.message_event.is_some() {
		return AppOutput::default();
	}
	let action = input.invocation.action.as_str();
	if !matches!(
		action,
		"view-raw" | "inspect" | "copy-json" | "copy-content"
	) {
		return AppOutput::default();
	}

	let app = input.app.as_ref();
	let timeline = app.and_then(|a| a.timeline.as_ref());
	let details: Option<&[MessageDetailSnapshot]> = app
		.and_then(|a| a.message_details.as_ref())
		.map(|d| d.items.as_slice());
	let rich: Option<&[RichMessageSnapshot]> = app
		.and_then(|a| a.message_content.as_ref())
		.map(|r| r.items.as_slice());

	let requested_id = input.invocation.value("message-id");
	let selected_text = input.invocation.selected_message.as_deref();
	let matched = timeline.and_then(|t| resolve_message(&t.messages, requested_id, selected_text));

	let target_id = matched.map(|m| m.id.as_str()).or_else(|| {
		requested_id
			.map(parse_option_id)
			.filter(|id| !id.is_empty())
	});
	let detail =
		target_id.and_then(|id| details.and_then(|items| items.iter().find(|d| d.id == id)));
	let rich_item =
		target_id.and_then(|id| rich.and_then(|items| items.iter().find(|r| r.id == id)));

	let channel_id = timeline
		.map(|t| t.channel_id.as_str())
		.or_else(|| {
			app.and_then(|a| a.message_details.as_ref())
				.map(|d| d.channel_id.as_str())
		})
		.or_else(|| {
			app.and_then(|a| a.message_content.as_ref())
				.map(|r| r.channel_id.as_str())
		})
		.or_else(|| {
			app.and_then(|a| a.context.as_ref())
				.and_then(|c| c.channel.as_ref())
				.map(|c| c.id.as_str())
		});

	let content = matched
		.map(|m| m.content.as_str())
		.or(selected_text)
		.unwrap_or("");
	let display_text = selected_text.filter(|text| *text != content);

	let raw = RawMessageView {
		id: target_id,
		channel_id,
		kind: detail.map(|d| d.kind),
		author: matched.map(|m| &m.author),
		content,
		display_text,
		edited: matched.map(|m| m.edited),
		attachment_count: matched.map(|m| m.attachment_count),
		reply_to: detail.and_then(|d| d.reply_to.as_deref()),
		reference: rich_item.and_then(|r| r.reference.as_ref()),
		mention_everyone: detail.map(|d| d.mention_everyone),
		mentions: detail
			.map(|d| d.mention_ids.iter().map(String::as_str).collect())
			.unwrap_or_default(),
		mentions_truncated: detail.is_some_and(|d| d.mentions_truncated),
		attachments: detail
			.map(|d| d.attachments.iter().collect())
			.unwrap_or_default(),
		attachments_truncated: detail.is_some_and(|d| d.attachments_truncated),
		embeds: rich_item
			.map(|r| r.embeds.iter().collect())
			.unwrap_or_default(),
		embeds_truncated: rich_item.is_some_and(|r| r.embeds_truncated),
		embeds_suppressed: rich_item.is_some_and(|r| r.embeds_suppressed),
		stickers: rich_item
			.map(|r| r.stickers.iter().collect())
			.unwrap_or_default(),
		stickers_truncated: rich_item.is_some_and(|r| r.stickers_truncated),
		reactions: detail.and_then(|d| d.reactions.as_deref()),
		reactions_truncated: detail.is_some_and(|d| d.reactions_truncated),
		poll: rich_item.map(|r| r.poll),
	};

	let json = serde_json::to_string_pretty(&raw).unwrap_or_else(|_| "{}".into());
	let panel_json = truncate_utf8(&json, MAX_PANEL_JSON_BYTES);

	let mut panel = vec![Element::Heading {
		text: match target_id {
			Some(id) => format!("View Raw · Message {id}"),
			None => "View Raw".into(),
		},
	}];

	let summary = match matched {
		Some(msg) => format!(
			"Author: {} ({}) · Channel: {} · Type: {} · Edited: {}\nAttachments: {} · Embeds: {}{} · Stickers: {} · Reactions: {}",
			msg.author.name,
			msg.author.id,
			channel_id.unwrap_or("unknown"),
			detail.map_or_else(|| "unknown".into(), |d| d.kind.to_string()),
			if msg.edited { "yes" } else { "no" },
			detail.map_or(usize::from(msg.attachment_count), |d| d.attachments.len()),
			rich_item.map_or(0, |r| r.embeds.len()),
			if rich_item.is_some_and(|r| r.embeds_suppressed) {
				" (suppressed)"
			} else {
				""
			},
			rich_item.map_or(0, |r| r.stickers.len()),
			detail
				.and_then(|d| d.reactions.as_ref())
				.map_or(0, Vec::len),
		),
		None => format!(
			"Channel: {} · Showing selected message text (full timeline metadata not in current window).",
			channel_id.unwrap_or("unknown")
		),
	};
	panel.push(Element::Text { text: summary });

	if let Some(timeline) = timeline
		&& !timeline.messages.is_empty()
	{
		let options: Vec<String> = timeline.messages.iter().rev().map(option_label).collect();
		let selected_option = matched
			.map(option_label)
			.unwrap_or_else(|| options[0].clone());
		panel.push(Element::Select {
			id: "message-id".into(),
			label: "Loaded message".into(),
			options,
			value: selected_option,
		});
	}

	panel.push(Element::Row {
		children: vec![
			Element::Button {
				id: "inspect".into(),
				label: "Inspect selected".into(),
			},
			Element::Button {
				id: "copy-json".into(),
				label: "Prepare Raw JSON".into(),
			},
			Element::Button {
				id: "copy-content".into(),
				label: "Prepare Raw Markdown".into(),
			},
		],
	});

	panel.push(Element::Separator);
	panel.push(Element::Heading {
		text: "Raw Markdown".into(),
	});
	panel.push(Element::Text {
		text: if content.is_empty() {
			"(empty message content)".into()
		} else {
			truncate_utf8(content, 4096)
		},
	});

	panel.push(Element::Separator);
	panel.push(Element::Heading {
		text: "Raw Message Data (JSON)".into(),
	});
	panel.push(Element::Text { text: panel_json });

	let clipboard_text = if action == "copy-content" {
		if content.is_empty() {
			selected_text.unwrap_or("")
		} else {
			content
		}
	} else {
		&json
	};
	let effects = if clipboard_text.is_empty() {
		Vec::new()
	} else {
		vec![HostEffect::CopyText {
			text: truncate_utf8(clipboard_text, MAX_CLIPBOARD_BYTES),
		}]
	};

	AppOutput {
		output: Output {
			panel,
			..Default::default()
		},
		effects,
	}
}

serein_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use serein_extension_sdk::{
		AppContextSnapshot, AppSnapshot, ChannelSnapshot, EmbedFieldSnapshot, Invocation,
		MessageContentSnapshot, MessageDetailsSnapshot, TimelineSnapshot, dispatch_typed,
	};

	fn sample_app() -> AppSnapshot {
		AppSnapshot {
			context: Some(AppContextSnapshot {
				connected: true,
				user: Some(UserSnapshot {
					id: "1".into(),
					name: "You".into(),
				}),
				channel: Some(ChannelSnapshot {
					id: "20".into(),
					guild_id: Some("10".into()),
					name: "general".into(),
					kind: 0,
				}),
			}),
			timeline: Some(TimelineSnapshot {
				channel_id: "20".into(),
				messages: vec![
					MessageSnapshot {
						id: "500".into(),
						author: UserSnapshot {
							id: "2".into(),
							name: "Robin".into(),
						},
						content: "**Hello** <#20>".into(),
						attachment_count: 1,
						edited: true,
					},
					MessageSnapshot {
						id: "501".into(),
						author: UserSnapshot {
							id: "3".into(),
							name: "Alex".into(),
						},
						content: String::new(),
						attachment_count: 0,
						edited: false,
					},
				],
				truncated: false,
			}),
			message_details: Some(MessageDetailsSnapshot {
				channel_id: "20".into(),
				items: vec![
					MessageDetailSnapshot {
						id: "500".into(),
						kind: 0,
						reply_to: Some("499".into()),
						mention_ids: vec!["1".into()],
						mentions_truncated: false,
						mention_everyone: false,
						attachments: vec![AttachmentSnapshot {
							id: "700".into(),
							filename: "notes.txt".into(),
							size: 128,
							content_type: Some("text/plain".into()),
							spoiler: false,
						}],
						attachments_truncated: false,
						reactions: Some(vec![ReactionSnapshot {
							emoji_id: None,
							emoji_name: Some("👍".into()),
							count: 2,
							me: true,
							me_burst: false,
						}]),
						reactions_truncated: false,
					},
					MessageDetailSnapshot {
						id: "501".into(),
						kind: 0,
						reply_to: None,
						mention_ids: Vec::new(),
						mentions_truncated: false,
						mention_everyone: false,
						attachments: Vec::new(),
						attachments_truncated: false,
						reactions: None,
						reactions_truncated: false,
					},
				],
				truncated: false,
			}),
			message_content: Some(MessageContentSnapshot {
				channel_id: "20".into(),
				items: vec![RichMessageSnapshot {
					id: "501".into(),
					embeds: vec![EmbedSummarySnapshot {
						kind: "rich".into(),
						title: Some("Embed Title".into()),
						description: Some("Embed Body".into()),
						author: Some("Bot".into()),
						footer: None,
						color: Some(0x5865f2),
						fields: vec![EmbedFieldSnapshot {
							name: "Field".into(),
							value: "Value".into(),
							inline: true,
						}],
						fields_truncated: false,
						has_image: true,
						has_thumbnail: false,
						has_video: false,
						limited: false,
					}],
					embeds_truncated: false,
					embeds_suppressed: false,
					stickers: Vec::new(),
					stickers_truncated: false,
					reference: None,
					poll: PollAvailability::Absent,
				}],
				truncated: false,
			}),
			..Default::default()
		}
	}

	#[test]
	fn right_click_view_raw_formats_markdown_and_json_and_proposes_clipboard_copy() {
		let mut values = std::collections::BTreeMap::new();
		values.insert("message-id".into(), "500".into());
		let input = AppInvocation {
			invocation: Invocation {
				action: "view-raw".into(),
				selected_message: Some("**Hello** <#20>".into()),
				values,
				..Default::default()
			},
			app: Some(sample_app()),
			..Default::default()
		};
		let wire = serde_json::to_vec(&input).unwrap();
		let out_bytes = dispatch_typed(&wire, handle).unwrap();
		let output: AppOutput = serde_json::from_slice(&out_bytes).unwrap();
		let rendered: String = output
			.output
			.panel
			.iter()
			.filter_map(|el| match el {
				Element::Text { text } | Element::Heading { text } => Some(text.as_str()),
				_ => None,
			})
			.collect::<Vec<_>>()
			.join("\n");
		assert!(rendered.contains("View Raw · Message 500"));
		assert!(rendered.contains("**Hello** <#20>"));
		assert!(rendered.contains("\"filename\": \"notes.txt\""));
		assert!(rendered.contains("\"reply_to\": \"499\""));
		assert_eq!(output.effects.len(), 1);
		match &output.effects[0] {
			HostEffect::CopyText { text } => {
				assert!(text.contains("\"id\": \"500\""));
			}
			other => panic!("unexpected effect: {other:?}"),
		}

		// Switching to message 501 via panel Select and copying raw markdown/JSON works.
		let mut inspect_values = std::collections::BTreeMap::new();
		inspect_values.insert("message-id".into(), "501 · Alex (no text)".into());
		let inspect_in = AppInvocation {
			invocation: Invocation {
				action: "inspect".into(),
				values: inspect_values,
				..Default::default()
			},
			app: Some(sample_app()),
			..Default::default()
		};
		let inspect_out: AppOutput = serde_json::from_slice(
			&dispatch_typed(&serde_json::to_vec(&inspect_in).unwrap(), handle).unwrap(),
		)
		.unwrap();
		let inspect_text: String = inspect_out
			.output
			.panel
			.iter()
			.filter_map(|el| match el {
				Element::Text { text } => Some(text.as_str()),
				_ => None,
			})
			.collect::<Vec<_>>()
			.join("\n");
		assert!(inspect_text.contains("\"title\": \"Embed Title\""));
	}
}
