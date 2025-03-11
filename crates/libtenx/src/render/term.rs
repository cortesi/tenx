use colored::CustomColor;
use colored::*;

use super::*;

/// Number of spaces to indent per level
const INDENT_SPACES: usize = 2;
/// Bullet character used in lists
const BULLET_CHAR: &str = "•";

/// Color for level 1 headers
const H1: &str = "#00D2D2";
/// Color for level 2 headers
const H2: &str = "#00B4B4";
/// Color for level 3+ headers
const H3: &str = "#FFCC00";

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
}

impl Term {
    pub fn new() -> Self {
        Self {
            level: 0,
            parts: Vec::new(),
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

impl Render for Term {
    fn push(&mut self, text: &str) {
        let header = match self.level {
            0 => text
                .bold()
                .custom_color(hex_to_custom_color(H1))
                .underline()
                .to_string(),
            1 => text.custom_color(hex_to_custom_color(H2)).to_string(),
            _ => text
                .bold()
                .custom_color(hex_to_custom_color(H3))
                .to_string(),
        };
        let header = format!("{}\n", header);

        self.add_indented(&header);
        self.level += 1;
    }

    fn pop(&mut self) {
        if self.level > 0 {
            self.level -= 1;
        }
    }

    fn para(&mut self, text: &str) {
        self.add_indented(text);
    }

    fn bullets(&mut self, items: Vec<String>) {
        for item in items {
            let bullet_line = format!("{} {}", BULLET_CHAR, item);
            self.add_indented(&bullet_line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Verify output contains expected number of lines
        let output = term.render();
        let lines: Vec<&str> = output.lines().collect();
        // Note: Colored strings contain ANSI escape sequences that may affect line counting
        assert_eq!(lines.len(), 9);

        // Get the indentation for each line
        let get_indent = |line: &str| {
            let mut spaces = 0;
            for c in line.chars() {
                if c == ' ' {
                    spaces += 1;
                } else {
                    break;
                }
            }
            spaces
        };

        println!("{}", output);

        // The actual output shown in the test shows we need to adjust our expectations
        // Check the first line (Main Title) - no indentation
        assert_eq!(get_indent(lines[0]), 0); // Level 0 has no indent

        // Looking at the actual output format, the blank line is index 1

        // Check subsequent lines with proper indentation
        assert_eq!(get_indent(lines[2]), 2); // This is a paragraph at level 1
        assert_eq!(get_indent(lines[3]), 2); // Bullet First item
        assert_eq!(get_indent(lines[4]), 2); // Bullet Second item
        assert_eq!(get_indent(lines[5]), 2); // Subtitle

        // Another blank line at index 6

        assert_eq!(get_indent(lines[7]), 4); // This is a paragraph at level 2
        assert_eq!(get_indent(lines[8]), 2); // Back to level 1
    }
}
