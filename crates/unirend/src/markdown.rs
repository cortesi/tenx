use super::Render;
use super::Style;

#[derive(Default)]
pub struct Markdown {
    level: usize,
    parts: Vec<String>,
}

impl Markdown {
    pub fn new() -> Self {
        Self {
            level: 0,
            parts: Vec::new(),
        }
    }

    pub fn render(&self) -> String {
        self.parts.join("\n")
    }
}

impl Render for Markdown {
    fn push(&mut self, text: &str) {
        let style = match self.level {
            0 => Style::H1,
            1 => Style::H2,
            2 => Style::H3,
            _ => Style::H4,
        };
        self.push_style(text, style);
    }

    fn push_style(&mut self, text: &str, style: Style) {
        let prefix = match style {
            Style::H1 => "# ",
            Style::H2 => "## ",
            Style::H3 => "### ",
            Style::H4 => "#### ",
            Style::Warn => "> ⚠️ ",
            Style::Error => "> ❌ ",
            Style::Success => "> ✅ ",
            Style::Plain => "",
        };

        self.parts.push(format!("{}{}", prefix, text));
        self.level += 1;
    }

    fn pop(&mut self) {
        if self.level > 0 {
            self.level -= 1;
        }
        // Add an empty line when popping a section
        self.parts.push(String::new());
    }

    fn para(&mut self, text: &str) {
        self.parts.push(text.to_string());
        // Add an empty line after each paragraph
        self.parts.push(String::new());
    }

    fn bullets(&mut self, items: Vec<String>) {
        for item in items {
            self.parts.push(format!("- {}", item));
        }
        // Add an empty line after the bullet list
        self.parts.push(String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_rendering() {
        let mut md = Markdown::new();

        // Level 0 header (H1)
        md.push("Main Title");

        // Paragraph at level 1
        md.para("This is a paragraph at level 1.");

        // Bullets at level 1
        md.bullets(vec!["First item".to_string(), "Second item".to_string()]);

        // Level 1 header (H2)
        md.push("Subtitle");

        // Paragraph at level 2
        md.para("This is a paragraph at level 2.");

        // Different styles
        md.push_style("Warning Section", Style::Warn);
        md.para("This is a warning paragraph.");

        // Pop back to level 2
        md.pop();

        // Pop back to level 1
        md.pop();

        // Paragraph at level 1 again
        md.para("Back to level 1.");

        // Render the output
        let output = md.render();

        // Check specific markdown formatting
        assert!(output.contains("# Main Title"));
        assert!(output.contains("This is a paragraph at level 1."));
        assert!(output.contains("- First item"));
        assert!(output.contains("- Second item"));
        assert!(output.contains("## Subtitle"));
        assert!(output.contains("This is a paragraph at level 2."));
        assert!(output.contains("> ⚠️ Warning Section"));
        assert!(output.contains("This is a warning paragraph."));
        assert!(output.contains("Back to level 1."));
    }
}
