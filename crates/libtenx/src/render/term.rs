use colored::*;

use super::*;

/// Number of spaces to indent per level
const INDENT_SPACES: usize = 2;
/// Bullet character used in lists
const BULLET_CHAR: &str = "•";

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
            0 => text.bold().bright_white().underline().to_string(),
            1 => text.bold().cyan().to_string(),
            _ => text.bold().yellow().to_string(),
        };

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
        assert_eq!(lines.len(), 7);

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

        // Verify indentation logic
        assert_eq!(get_indent(lines[0]), 0); // Level 0 has no indent
        assert_eq!(get_indent(lines[1]), 2); // Level 1 has 2 spaces
        assert_eq!(get_indent(lines[2]), 2); // Bullets at level 1
        assert_eq!(get_indent(lines[3]), 2); // Bullets at level 1
        assert_eq!(get_indent(lines[4]), 2); // Subtitle
        assert_eq!(get_indent(lines[5]), 4); // Paragraph at level 2
        assert_eq!(get_indent(lines[6]), 2); // Back to level 1
    }
}

