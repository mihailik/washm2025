mod columnar_menu;
mod description_menu;
mod ide_menu;
mod list_menu;
pub mod menu_functions;

use crate::core_editor::Editor;
use crate::History;
use crate::{completion::history::HistoryCompleter, painting::Painter, Completer, Suggestion};
use std::future::Future;
use std::io::Write;
use std::pin::Pin;

pub type MenuFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub use columnar_menu::ColumnarMenu;
pub use columnar_menu::TraversalDirection;
pub use description_menu::DescriptionMenu;
pub use ide_menu::DescriptionMode;
pub use ide_menu::IdeMenu;
pub use list_menu::ListMenu;
use nu_ansi_term::{Color, Style};

/// Struct to store the menu style
pub struct MenuTextStyle {
    /// Text style for selected text in a menu
    pub selected_text_style: Style,
    /// Text style for not selected text in the menu
    pub text_style: Style,
    /// Text style for the item description
    pub description_style: Style,
    /// Text style of the parts of the suggestions that match the
    /// typed text when the suggestion is selected
    pub selected_match_style: Style,
    /// Text style of the parts of the suggestions that match the
    /// typed text
    pub match_style: Style,
}

impl Default for MenuTextStyle {
    fn default() -> Self {
        Self {
            selected_text_style: Color::Green.bold().reverse(),
            text_style: Color::DarkGray.normal(),
            description_style: Color::Yellow.normal(),
            selected_match_style: Color::Green.bold().reverse().underline(),
            match_style: Style::default().underline(),
        }
    }
}

/// Defines all possible events that could happen with a menu.
#[derive(Clone)]
pub enum MenuEvent {
    /// Activation event for the menu. When the bool is true it means that the values
    /// have already being updated. This is true when the option `quick_completions` is true
    Activate(bool),
    /// Deactivation event
    Deactivate,
    /// Line buffer edit event. When the bool is true it means that the values
    /// have already being updated. This is true when the option `quick_completions` is true
    Edit(bool),
    /// Selecting next element in the menu
    NextElement,
    /// Selecting previous element in the menu
    PreviousElement,
    /// Moving up in the menu
    MoveUp,
    /// Moving down in the menu
    MoveDown,
    /// Moving left in the menu
    MoveLeft,
    /// Moving right in the menu
    MoveRight,
    /// Move to next page
    NextPage,
    /// Move to previous page
    PreviousPage,
}

/// Trait that defines how a menu will be printed by the painter
pub trait Menu {
    /// Get MenuSettings
    fn settings(&self) -> &MenuSettings {
        // We panic here, so this function has base implementation
        // so existing menus will not break.
        // if a breaking change is ok, this can be removed
        panic!("`settings` requires a manual implementation per menu. It has a base implementation to not break existing menus")
    }

    /// Menu name
    fn name(&self) -> &str {
        &self.settings().name
    }

    /// Menu indicator
    fn indicator(&self) -> &str {
        &self.settings().marker
    }

    /// Checks if the menu is active
    fn is_active(&self) -> bool;

    /// Selects what type of event happened with the menu
    fn menu_event(&mut self, event: MenuEvent);

    /// A menu may not be allowed to quick complete because it needs to stay
    /// active even with one element
    fn can_quick_complete(&self) -> bool;

    /// The completion menu can try to find the common string and replace it
    /// in the given line buffer
    fn can_partially_complete<'a>(
        &'a mut self,
        values_updated: bool,
        editor: &'a mut Editor,
        completer: &'a mut dyn Completer,
    ) -> MenuFuture<'a, bool>;

    /// Updates the values presented in the menu
    /// This function needs to be defined in the trait because when the menu is
    /// activated or the `quick_completion` option is true, the len of the values
    /// is calculated to know if there is only one value so it can be selected
    /// immediately
    fn update_values<'a>(
        &'a mut self,
        editor: &'a mut Editor,
        completer: &'a mut dyn Completer,
    ) -> MenuFuture<'a, ()>;

    /// The working details of a menu are values that could change based on
    /// the menu conditions before it being printed, such as the number or size
    /// of columns, etc.
    /// In this function should be defined how the menu event is treated since
    /// it is called just before painting the menu
    fn update_working_details<'a, W: Write>(
        &'a mut self,
        _editor: &'a mut Editor,
        _completer: &'a mut dyn Completer,
        _painter: &'a Painter<W>,
    ) -> MenuFuture<'a, ()>
    where
        Self: Sized;

    fn update_working_details_dyn<'a>(
        &'a mut self,
        editor: &'a mut Editor,
        completer: &'a mut dyn Completer,
        painter: &'a Painter<Box<dyn Write + Send>>,
    ) -> MenuFuture<'a, ()>;

    /// Indicates how to replace in the line buffer the selected value from the menu
    fn replace_in_buffer(&self, editor: &mut Editor);

    /// Calculates the real required lines for the menu considering how many lines
    /// wrap the terminal or if entries have multiple lines
    fn menu_required_lines(&self, terminal_columns: u16) -> u16;

    /// Creates the menu representation as a string which will be painted by the painter
    fn menu_string(&self, available_lines: u16, use_ansi_coloring: bool) -> String;

    /// Minimum rows that should be displayed by the menu
    fn min_rows(&self) -> u16;

    /// Gets cached values from menu that will be displayed
    fn get_values(&self) -> &[Suggestion];
    /// Sets the position of the cursor (currently only required by the IDE menu)
    fn set_cursor_pos(&mut self, _pos: (u16, u16)) {
        // empty implementation to make it optional
    }
}

/// Struct to store configuration for a menu.
pub struct MenuSettings {
    /// Menu name
    name: String,
    /// Menu coloring
    color: MenuTextStyle,
    /// Menu marker when active
    marker: String,
    /// Calls the completer using only the line buffer difference difference
    /// after the menu was activated
    only_buffer_difference: bool,
}

impl Default for MenuSettings {
    fn default() -> Self {
        Self {
            name: "menu".to_string(),
            color: MenuTextStyle::default(),
            marker: "| ".to_string(),
            only_buffer_difference: false,
        }
    }
}

impl MenuSettings {
    /// MenuSettings builder with name
    #[must_use]
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    /// MenuSettings builder with color
    #[must_use]
    pub fn with_color(mut self, color: MenuTextStyle) -> Self {
        self.color = color;
        self
    }

    /// MenuSettings builder with marker
    #[must_use]
    pub fn with_marker(mut self, marker: &str) -> Self {
        self.marker = marker.to_string();
        self
    }

    /// MenuSettings builder with only_buffer_difference
    #[must_use]
    pub fn with_only_buffer_difference(mut self, only_buffer_difference: bool) -> Self {
        self.only_buffer_difference = only_buffer_difference;
        self
    }
}

/// Common builder for all menus
pub trait MenuBuilder: Menu + Sized {
    /// Get mutable MenuSettings
    /// required for the builder functions
    fn settings_mut(&mut self) -> &mut MenuSettings;

    /// Menu builder with new name
    #[must_use]
    fn with_name(mut self, name: &str) -> Self {
        self.settings_mut().name = name.to_string();
        self
    }

    /// Menu builder with new value for text style
    #[must_use]
    fn with_text_style(mut self, color: Style) -> Self {
        self.settings_mut().color.text_style = color;
        self
    }

    /// Menu builder with new value for selected text style
    #[must_use]
    fn with_selected_text_style(mut self, color: Style) -> Self {
        self.settings_mut().color.selected_text_style = color;
        self
    }

    /// Menu builder with new value for description style
    #[must_use]
    fn with_description_text_style(mut self, color: Style) -> Self {
        self.settings_mut().color.description_style = color;
        self
    }

    /// Menu builder with new value for match style
    /// This is the style of the part of the input text, the suggestions
    /// are based on
    #[must_use]
    fn with_match_text_style(mut self, color: Style) -> Self {
        self.settings_mut().color.match_style = color;
        self
    }

    /// Menu builder with new value for selected match style
    /// This is the style of the part of the input text, the suggestions
    /// are based on
    #[must_use]
    fn with_selected_match_text_style(mut self, color: Style) -> Self {
        self.settings_mut().color.selected_match_style = color;
        self
    }

    /// Menu builder with new value for marker
    #[must_use]
    fn with_marker(mut self, marker: &str) -> Self {
        self.settings_mut().marker = marker.to_string();
        self
    }

    /// Menu builder with new value for only_buffer_difference
    #[must_use]
    fn with_only_buffer_difference(mut self, only_buffer_difference: bool) -> Self {
        self.settings_mut().only_buffer_difference = only_buffer_difference;
        self
    }
}

/// Allowed menus in Reedline
pub enum ReedlineMenu {
    /// Menu that uses Reedline's completer to update its values
    EngineCompleter(Box<dyn Menu>),
    /// Menu that uses the history as its completer
    HistoryMenu(Box<dyn Menu>),
    /// Menu that has its own Completer
    WithCompleter {
        /// Base menu
        menu: Box<dyn Menu>,
        /// External completer defined outside Reedline
        completer: Box<dyn Completer>,
    },
}

impl ReedlineMenu {
    pub(crate) async fn can_partially_complete(
        &mut self,
        values_updated: bool,
        editor: &mut Editor,
        completer: &mut dyn Completer,
        history: &dyn History,
    ) -> bool {
        match self {
            Self::EngineCompleter(menu) => {
                menu.can_partially_complete(values_updated, editor, completer)
                    .await
            }
            Self::HistoryMenu(menu) => {
                let mut history_completer = HistoryCompleter::new(history);
                menu.can_partially_complete(values_updated, editor, &mut history_completer)
                    .await
            }
            Self::WithCompleter {
                menu,
                completer: own_completer,
            } => {
                menu.can_partially_complete(values_updated, editor, own_completer.as_mut())
                    .await
            }
        }
    }

    pub(crate) async fn update_values(
        &mut self,
        editor: &mut Editor,
        completer: &mut dyn Completer,
        history: &dyn History,
    ) {
        match self {
            Self::EngineCompleter(menu) => menu.update_values(editor, completer).await,
            Self::HistoryMenu(menu) => {
                let mut history_completer = HistoryCompleter::new(history);
                menu.update_values(editor, &mut history_completer).await;
            }
            Self::WithCompleter {
                menu,
                completer: own_completer,
            } => {
                menu.update_values(editor, own_completer.as_mut()).await;
            }
        }
    }

    pub(crate) async fn update_working_details(
        &mut self,
        editor: &mut Editor,
        completer: &mut dyn Completer,
        history: &dyn History,
    ) {
        match self {
            Self::EngineCompleter(menu) => {
                menu.update_working_details_dyn(editor, completer, &Painter::new(Box::new(std::io::sink()) as Box<dyn Write + Send>))
                    .await;
            }
            Self::HistoryMenu(menu) => {
                let mut history_completer = HistoryCompleter::new(history);
                menu.update_working_details_dyn(editor, &mut history_completer, &Painter::new(Box::new(std::io::sink()) as Box<dyn Write + Send>))
                    .await;
            }
            Self::WithCompleter {
                menu,
                completer: own_completer,
            } => {
                menu.update_working_details_dyn(editor, own_completer.as_mut(), &Painter::new(Box::new(std::io::sink()) as Box<dyn Write + Send>))
                    .await;
            }
        }
    }

    pub fn set_cursor_pos(&mut self, pos: (u16, u16)) {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.set_cursor_pos(pos),
        }
    }

    pub fn replace_in_buffer(&self, editor: &mut Editor) {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.replace_in_buffer(editor),
        }
    }

    pub fn menu_required_lines(&self, terminal_columns: u16) -> u16 {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.menu_required_lines(terminal_columns),
        }
    }

    pub fn menu_string(&self, available_lines: u16, use_ansi_coloring: bool) -> String {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => {
                menu.menu_string(available_lines, use_ansi_coloring)
            }
        }
    }

    pub fn min_rows(&self) -> u16 {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.min_rows(),
        }
    }

    pub fn get_values(&self) -> &[Suggestion] {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.get_values(),
        }
    }

    pub fn settings(&self) -> &MenuSettings {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.settings(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.name(),
        }
    }

    pub fn indicator(&self) -> &str {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.indicator(),
        }
    }

    pub fn is_active(&self) -> bool {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.is_active(),
        }
    }

    pub fn menu_event(&mut self, event: MenuEvent) {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.menu_event(event),
        }
    }

    pub fn can_quick_complete(&self) -> bool {
        match self {
            Self::EngineCompleter(menu)
            | Self::HistoryMenu(menu)
            | Self::WithCompleter { menu, .. } => menu.can_quick_complete(),
        }
    }
}

