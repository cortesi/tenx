#![allow(dead_code)]
use colored::CustomColor;
use colored::*;
use terminal_size::{terminal_size, Height, Width};
use textwrap;

// Import our Style enum explicitly to avoid ambiguity with colored::Style
use super::Render;
use super::Style as RenderStyle;

/// Number of spaces to indent per level
const INDENT_SPACES: usize = 2;
/// Bullet character used in lists
const BULLET_CHAR: &str = "•";

// Solarized color scheme
const BASE03: &str = "#002b36";
const BASE02: &str = "#073642";
const BASE01: &str = "#586e75";
const BASE00: &str = "#657b83";
const BASE0: &str = "#839496";
const BASE1: &str = "#93a1a1";
const BASE2: &str = "#eee8d5";
const BASE3: &str = "#fdf6e3";
const YELLOW: &str = "#b58900";
const ORANGE: &str = "#cb4b16";
const RED: &str = "#dc322f";
const MAGENTA: &str = "#d33682";
const VIOLET: &str = "#6c71c4";
const BLUE: &str = "#268bd2";
const CYAN: &str = "#2aa198";
const GREEN: &str = "#859900";

/// Foreground color for level 1 headers
const H1_FG: &str = YELLOW;
/// Background color for level 1 headers
const H1_BG: &str = BASE02;
/// Color for level 2 headers
const H2_FG: &str = BLUE;
/// Background color for level 2 headers
const H2_BG: &str = "";
/// Color for level 3+ headers
const H3_FG: &str = CYAN;
/// Background color for level 3+ headers
const H3_BG: &str = "";

// Style-specific colors
const WARN_FG: &str = ORANGE;
const WARN_BG: &str = "";
const ERROR_FG: &str = RED;
const ERROR_BG: &str = "";
const SUCCESS_FG: &str = GREEN;
const SUCCESS_BG: &str = "";
const DEFAULT_FG: &str = BASE1;
const DEFAULT_BG: &str = "";

/// Default width when not in a terminal
const DEFAULT_WIDTH: usize = 100;

fn right_pad(s: &str, width: usize) -> String {
    let mut padded = s.to_string();
    let padding = width.saturating_sub(s.len());
    padded.push_str(&" ".repeat(padding));
    padded
}

/// Convert a hex color string (#RRGGBB) to a CustomColor
fn hex_to_custom_color(hex: &str) -> CustomColor {
    // Remove the leading # if present
    let hex = hex.trim_start_matches('#');

    // Parse the hex values
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);

    CustomColor { r, g, b }
}

pub struct Term {
    level: usize,
    parts: Vec<String>,
    width: usize,
}

impl Term {
    pub fn new() -> Self {
        // Get terminal width using terminal_size crate
        let width = terminal_size()
            .map(|(Width(w), Height(_))| w as usize)
            .unwrap_or(DEFAULT_WIDTH);

        Self {
            level: 0,
            parts: Vec::new(),
            width,
        }
    }

    pub fn render(&self) -> String {
        self.parts.join("\n")
    }

    /// Adds a line with the appropriate indentation to parts
    fn add_indented(&mut self, text: &str) {
        let indent = " ".repeat(self.level * INDENT_SPACES);
        self.parts.push(format!("{}{}", indent, text));
    }
}

impl Default for Term {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply styling to text based on a RenderStyle enum
fn apply_style(text: &str, style: &RenderStyle) -> String {
    let (fg_color, bg_color, make_bold) = match style {
        RenderStyle::H1 => (H1_FG, H1_BG, true),
        RenderStyle::H2 => (H2_FG, H2_BG, false),
        RenderStyle::H3 | RenderStyle::H4 => (H3_FG, H3_BG, false),
        RenderStyle::Warn => (WARN_FG, WARN_BG, true),
        RenderStyle::Error => (ERROR_FG, ERROR_BG, true),
        RenderStyle::Success => (SUCCESS_FG, SUCCESS_BG, true),
        RenderStyle::Plain => (DEFAULT_FG, DEFAULT_BG, false),
    };

    let mut styled = text.custom_color(hex_to_custom_color(fg_color));

    if make_bold {
        styled = styled.bold();
    }

    if !bg_color.is_empty() {
        styled = styled.on_custom_color(hex_to_custom_color(bg_color));
    }

    styled.to_string()
}

impl Render for Term {
    #[allow(clippy::const_is_empty)]
    fn push(&mut self, text: &str) {
        let style = match self.level {
            0 => RenderStyle::H1,
            1 => RenderStyle::H2,
            _ => RenderStyle::H3,
        };
        self.push_style(text, style);
    }

    fn push_style(&mut self, text: &str, style: RenderStyle) {
        // Calculate the available width for wrapping text
        let indent_width = self.level * INDENT_SPACES;
        let available_width = if self.width > indent_width {
            self.width - indent_width
        } else {
            self.width
        };
        let text = right_pad(text, available_width - indent_width);

        // Apply styling based on the provided style
        let styled_text = apply_style(&text, &style);

        // Wrap the header text
        self.add_indented(&styled_text);
        self.level += 1;
    }

    fn pop(&mut self) {
        if self.level > 0 {
            self.level -= 1;
        }
    }

    fn para(&mut self, text: &str) {
        // Calculate the available width for wrapping text
        // Account for indentation to prevent text from wrapping incorrectly
        let indent_width = self.level * INDENT_SPACES;
        let available_width = if self.width > indent_width {
            self.width - indent_width
        } else {
            self.width
        };

        // Wrap the text to the available width
        let wrapped_text = textwrap::fill(text, available_width);

        // Add each wrapped line with proper indentation
        for line in wrapped_text.lines() {
            self.add_indented(line);
        }

        // Add an extra newline
        self.parts.push("".to_string());
    }

    fn bullets(&mut self, items: Vec<String>) {
        // Calculate the available width for wrapping text, accounting for indentation
        let indent_width = self.level * INDENT_SPACES;
        let bullet_prefix_width = 2; // BULLET_CHAR plus space
        let available_width = if self.width > (indent_width + bullet_prefix_width) {
            self.width - indent_width - bullet_prefix_width
        } else {
            self.width
        };

        for item in items {
            // Wrap the bullet item text
            let wrapped_text = textwrap::fill(&item, available_width);
            let wrapped_lines: Vec<&str> = wrapped_text.lines().collect();

            // First line with bullet
            if !wrapped_lines.is_empty() {
                let first_line = format!("{} {}", BULLET_CHAR, wrapped_lines[0]);
                self.add_indented(&first_line);

                // Subsequent lines with indent aligned with text after bullet
                let continuation_indent = " ".repeat(bullet_prefix_width);
                for line in wrapped_lines.iter().skip(1) {
                    let indented_line = format!("{}{}", continuation_indent, line);
                    self.add_indented(&indented_line);
                }
            }
        }

        // Add an extra newline after all bullets
        self.parts.push("".to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Macro for asserting the indentation of a line containing specific text
    macro_rules! assert_indent {
        ($output:expr, $text:expr, $expected_indent:expr) => {
            let lines: Vec<&str> = $output.lines().collect();
            let line = lines
                .iter()
                .find(|line| line.contains($text))
                .unwrap_or_else(|| panic!("Text '{}' not found in output", $text));

            let indent = line.chars().take_while(|&c| c == ' ').count();

            assert_eq!(
                indent, $expected_indent,
                "Expected indentation of {} for text '{}', but got {}",
                $expected_indent, $text, indent
            );
        };
    }

    #[test]
    fn test_term_rendering() {
        let mut term = Term::new();

        // Level 0 header
        term.push("Main Title");

        // Paragraph at level 1
        term.para("This is a paragraph at level 1.");

        // Bullets at level 1
        term.bullets(vec!["First item".to_string(), "Second item".to_string()]);

        // Level 1 header
        term.push("Subtitle");

        // Paragraph at level 2
        term.para("This is a paragraph at level 2.");

        // Pop back to level 1
        term.pop();

        // Paragraph at level 1 again
        term.para("Back to level 1.");

        // Render the output
        let output = term.render();

        // Test indentation using the macro
        assert_indent!(output, "Main Title", 0);
        assert_indent!(output, "This is a paragraph at level 1", 2);
        assert_indent!(output, "First item", 2);
        assert_indent!(output, "Second item", 2);
        assert_indent!(output, "Subtitle", 2);
        assert_indent!(output, "This is a paragraph at level 2", 4);
        assert_indent!(output, "Back to level 1", 2);
    }
}
