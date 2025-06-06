use crate::output::Output;
use crate::panes::PaneId;
use crate::tab::Pane;
use crate::ui::boundaries::Boundaries;
use crate::ui::pane_boundaries_frame::FrameParams;
use crate::ClientId;
use std::collections::{HashMap, HashSet};
use zellij_utils::data::{client_id_to_colors, InputMode, PaletteColor, Style};
use zellij_utils::errors::prelude::*;
pub struct PaneContentsAndUi<'a> {
    pane: &'a mut Box<dyn Pane>,
    output: &'a mut Output,
    style: Style,
    focused_clients: Vec<ClientId>,
    multiple_users_exist_in_session: bool,
    z_index: Option<usize>,
    pane_is_stacked_under: bool,
    pane_is_stacked_over: bool,
    should_draw_pane_frames: bool,
    mouse_is_hovering_over_pane_for_clients: HashSet<ClientId>,
    current_pane_group: HashMap<ClientId, Vec<PaneId>>,
}

impl<'a> PaneContentsAndUi<'a> {
    pub fn new(
        pane: &'a mut Box<dyn Pane>,
        output: &'a mut Output,
        style: Style,
        active_panes: &HashMap<ClientId, PaneId>,
        multiple_users_exist_in_session: bool,
        z_index: Option<usize>,
        pane_is_stacked_under: bool,
        pane_is_stacked_over: bool,
        should_draw_pane_frames: bool,
        mouse_hover_pane_id: &HashMap<ClientId, PaneId>,
        current_pane_group: HashMap<ClientId, Vec<PaneId>>,
    ) -> Self {
        let mut focused_clients: Vec<ClientId> = active_panes
            .iter()
            .filter(|(_c_id, p_id)| **p_id == pane.pid())
            .map(|(c_id, _p_id)| *c_id)
            .collect();
        focused_clients.sort_unstable();
        let mouse_is_hovering_over_pane_for_clients = mouse_hover_pane_id
            .iter()
            .filter_map(|(client_id, pane_id)| {
                if pane_id == &pane.pid() {
                    Some(*client_id)
                } else {
                    None
                }
            })
            .collect();
        PaneContentsAndUi {
            pane,
            output,
            style,
            focused_clients,
            multiple_users_exist_in_session,
            z_index,
            pane_is_stacked_under,
            pane_is_stacked_over,
            should_draw_pane_frames,
            mouse_is_hovering_over_pane_for_clients,
            current_pane_group,
        }
    }
    pub fn render_pane_contents_to_multiple_clients(
        &mut self,
        clients: impl Iterator<Item = ClientId>,
    ) -> Result<()> {
        let err_context = "failed to render pane contents to multiple clients";

        // here we drop the fake cursors so that their lines will be updated
        // and we can clear them from the UI below
        drop(self.pane.drain_fake_cursors());

        if let Some((character_chunks, raw_vte_output, sixel_image_chunks)) =
            self.pane.render(None).context(err_context)?
        {
            let clients: Vec<ClientId> = clients.collect();
            self.output
                .add_character_chunks_to_multiple_clients(
                    character_chunks,
                    clients.iter().copied(),
                    self.z_index,
                )
                .context(err_context)?;
            self.output.add_sixel_image_chunks_to_multiple_clients(
                sixel_image_chunks,
                clients.iter().copied(),
                self.z_index,
            );
            if let Some(raw_vte_output) = raw_vte_output {
                if !raw_vte_output.is_empty() {
                    self.output.add_post_vte_instruction_to_multiple_clients(
                        clients.iter().copied(),
                        &format!(
                            "\u{1b}[{};{}H\u{1b}[m{}",
                            self.pane.y() + 1,
                            self.pane.x() + 1,
                            raw_vte_output
                        ),
                    );
                }
            }
        }
        Ok(())
    }
    pub fn render_pane_contents_for_client(&mut self, client_id: ClientId) -> Result<()> {
        let err_context = || format!("failed to render pane contents for client {client_id}");

        if let Some((character_chunks, raw_vte_output, sixel_image_chunks)) = self
            .pane
            .render(Some(client_id))
            .with_context(err_context)?
        {
            self.output
                .add_character_chunks_to_client(client_id, character_chunks, self.z_index)
                .with_context(err_context)?;
            self.output.add_sixel_image_chunks_to_client(
                client_id,
                sixel_image_chunks,
                self.z_index,
            );
            if let Some(raw_vte_output) = raw_vte_output {
                self.output.add_post_vte_instruction_to_client(
                    client_id,
                    &format!(
                        "\u{1b}[{};{}H\u{1b}[m{}",
                        self.pane.y() + 1,
                        self.pane.x() + 1,
                        raw_vte_output
                    ),
                );
            }
        }
        Ok(())
    }
    pub fn render_fake_cursor_if_needed(&mut self, client_id: ClientId) -> Result<()> {
        let pane_focused_for_client_id = self.focused_clients.contains(&client_id);
        let pane_focused_for_different_client = self
            .focused_clients
            .iter()
            .filter(|&&c_id| c_id != client_id)
            .count()
            > 0;
        if pane_focused_for_different_client && !pane_focused_for_client_id {
            let fake_cursor_client_id = self
                .focused_clients
                .iter()
                .find(|&&c_id| c_id != client_id)
                .with_context(|| {
                    format!("failed to render fake cursor if needed for client {client_id}")
                })?;
            if let Some(colors) = client_id_to_colors(
                *fake_cursor_client_id,
                self.style.colors.multiplayer_user_colors,
            ) {
                let cursor_is_visible = self
                    .pane
                    .cursor_coordinates()
                    .map(|(x, y)| {
                        self.output
                            .cursor_is_visible(self.pane.x() + x, self.pane.y() + y)
                    })
                    .unwrap_or(false);
                if cursor_is_visible {
                    if let Some(vte_output) = self.pane.render_fake_cursor(colors.0, colors.1) {
                        self.output.add_post_vte_instruction_to_client(
                            client_id,
                            &format!(
                                "\u{1b}[{};{}H\u{1b}[m{}",
                                self.pane.y() + 1,
                                self.pane.x() + 1,
                                vte_output
                            ),
                        );
                    }
                }
            }
        }
        Ok(())
    }
    pub fn render_terminal_title_if_needed(
        &mut self,
        client_id: ClientId,
        client_mode: InputMode,
        previous_title: &mut Option<String>,
    ) {
        if !self.focused_clients.contains(&client_id) {
            return;
        }
        let vte_output = self.pane.render_terminal_title(client_mode);
        if let Some(previous_title) = previous_title {
            if *previous_title == vte_output {
                return;
            }
        }
        *previous_title = Some(vte_output.clone());
        self.output
            .add_post_vte_instruction_to_client(client_id, &vte_output);
    }
    pub fn render_pane_frame(
        &mut self,
        client_id: ClientId,
        client_mode: InputMode,
        session_is_mirrored: bool,
        pane_is_floating: bool,
        pane_is_selectable: bool,
    ) -> Result<()> {
        let err_context = || format!("failed to render pane frame for client {client_id}");

        let pane_focused_for_client_id = self.focused_clients.contains(&client_id);
        let other_focused_clients: Vec<ClientId> = self
            .focused_clients
            .iter()
            .filter(|&&c_id| c_id != client_id)
            .copied()
            .collect();
        let pane_focused_for_differet_client = !other_focused_clients.is_empty();

        let frame_color = self.frame_color(client_id, client_mode, session_is_mirrored);
        let focused_client = if pane_focused_for_client_id {
            Some(client_id)
        } else if pane_focused_for_differet_client {
            Some(*other_focused_clients.first().with_context(err_context)?)
        } else {
            None
        };
        let frame_params = if session_is_mirrored {
            FrameParams {
                focused_client,
                is_main_client: pane_focused_for_client_id,
                other_focused_clients: vec![],
                style: self.style,
                color: frame_color.map(|c| c.0),
                other_cursors_exist_in_session: false,
                pane_is_stacked_over: self.pane_is_stacked_over,
                pane_is_stacked_under: self.pane_is_stacked_under,
                should_draw_pane_frames: self.should_draw_pane_frames,
                pane_is_floating,
                content_offset: self.pane.get_content_offset(),
                mouse_is_hovering_over_pane: self
                    .mouse_is_hovering_over_pane_for_clients
                    .contains(&client_id),
                pane_is_selectable,
            }
        } else {
            FrameParams {
                focused_client,
                is_main_client: pane_focused_for_client_id,
                other_focused_clients,
                style: self.style,
                color: frame_color.map(|c| c.0),
                other_cursors_exist_in_session: self.multiple_users_exist_in_session,
                pane_is_stacked_over: self.pane_is_stacked_over,
                pane_is_stacked_under: self.pane_is_stacked_under,
                should_draw_pane_frames: self.should_draw_pane_frames,
                pane_is_floating,
                content_offset: self.pane.get_content_offset(),
                mouse_is_hovering_over_pane: self
                    .mouse_is_hovering_over_pane_for_clients
                    .contains(&client_id),
                pane_is_selectable,
            }
        };

        if let Some((frame_terminal_characters, vte_output)) = self
            .pane
            .render_frame(client_id, frame_params, client_mode)
            .with_context(err_context)?
        {
            self.output
                .add_character_chunks_to_client(client_id, frame_terminal_characters, self.z_index)
                .with_context(err_context)?;
            if let Some(vte_output) = vte_output {
                self.output
                    .add_post_vte_instruction_to_client(client_id, &vte_output);
            }
        }
        Ok(())
    }
    pub fn render_pane_boundaries(
        &self,
        client_id: ClientId,
        client_mode: InputMode,
        boundaries: &mut Boundaries,
        session_is_mirrored: bool,
        pane_is_on_top_of_stack: bool,
        pane_is_on_bottom_of_stack: bool,
    ) {
        let color = self.frame_color(client_id, client_mode, session_is_mirrored);
        boundaries.add_rect(
            self.pane.as_ref(),
            color,
            pane_is_on_top_of_stack,
            pane_is_on_bottom_of_stack,
            self.pane_is_stacked_under,
        );
    }
    fn frame_color(
        &self,
        client_id: ClientId,
        mode: InputMode,
        session_is_mirrored: bool,
    ) -> Option<(PaletteColor, usize)> {
        // (color, color_precedence) (the color_precedence is used
        // for the no-pane-frames mode)
        let pane_focused_for_client_id = self.focused_clients.contains(&client_id);
        let pane_is_in_group = self
            .current_pane_group
            .get(&client_id)
            .map(|p| p.contains(&self.pane.pid()))
            .unwrap_or(false);
        if self.pane.frame_color_override().is_some() && !pane_is_in_group {
            self.pane
                .frame_color_override()
                .map(|override_color| (override_color, 4))
        } else if pane_is_in_group && !pane_focused_for_client_id {
            Some((self.style.colors.frame_highlight.emphasis_0, 2))
        } else if pane_is_in_group && pane_focused_for_client_id {
            Some((self.style.colors.frame_highlight.emphasis_1, 3))
        } else if pane_focused_for_client_id {
            match mode {
                InputMode::Normal | InputMode::Locked => {
                    if session_is_mirrored || !self.multiple_users_exist_in_session {
                        Some((self.style.colors.frame_selected.base, 3))
                    } else {
                        let colors = client_id_to_colors(
                            client_id,
                            self.style.colors.multiplayer_user_colors,
                        );
                        colors.map(|colors| (colors.0, 3))
                    }
                },
                _ => Some((self.style.colors.frame_highlight.base, 3)),
            }
        } else if self
            .mouse_is_hovering_over_pane_for_clients
            .contains(&client_id)
        {
            Some((self.style.colors.frame_highlight.base, 1))
        } else {
            self.style
                .colors
                .frame_unselected
                .map(|frame| (frame.base, 0))
        }
    }
}
