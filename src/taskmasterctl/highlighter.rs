extern crate syntect;

use syntect::easy::HighlightLines;
use syntect::parsing::{SyntaxSet, SyntaxDefinition, Scope, ScopeStack, SyntaxSetBuilder};
use syntect::highlighting::{Style, Theme, StyleModifier, Color, ThemeItem, ScopeSelector, ScopeSelectors, ThemeSettings};
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

fn rgb(color: u64) -> Option<Color> {
	Some(Color{
		r: ((color >> 16) & 0xff) as u8,
		g: ((color >> 8) & 0xff) as u8,
		b: ((color >> 0) & 0xff) as u8,
		a: 255
	})
}

fn theme_item(scope: &str, color: u64) -> ThemeItem {
	ThemeItem {
		scope: ScopeSelectors { 
			selectors: vec![ScopeSelector {
				path: ScopeStack::from_vec(vec![Scope::new(scope).unwrap()]),
				..Default::default()
			}]
		},
		style: StyleModifier {
			foreground: rgb(color),
			..Default::default()
		}
	}
}

fn create_syntect_theme() -> Theme {
	Theme {
		scopes: vec![
			theme_item("function", 0x73d0ff),
			theme_item("keyword", 0xffad66),
			theme_item("string", 0xd5ff80),
			theme_item("string.escape", 0xdd5555),
			theme_item("flag", 0xffd173),
			theme_item("number", 0xdfbfff),
		],
		settings: ThemeSettings {
			foreground: rgb(0xffffff),
			..Default::default()
		},
		..Default::default()
	}
}

fn create_syntect_syntax() -> SyntaxDefinition {
	SyntaxDefinition::load_from_str(
r#"name: taskmasterctl
file_extensions: []
scope: taskmasterctl

contexts:
  main:
  - match: \b(help|status|start|stop|restart|info|load|unload|reload|logs)\b
    scope: function
  - match: \bglobal\b
    scope: keyword
  - match: '"'
    push: string
  - match: \s[+-]?([0-9_]+)\b
    scope: number
  - match: \s(-[a-zA-Z-]+)\b
    scope: flag

  string:
  - meta_scope: string
  - match: \\.
    scope: string.escape
  - match: '"'
    pop: true"#, false, None).unwrap()
}

pub struct TaskmasterHighlighter {
	theme: Theme,
	syntax_set: SyntaxSet,
}

impl TaskmasterHighlighter {
	pub fn new() -> TaskmasterHighlighter {
		let theme = create_syntect_theme();
		let syntax = create_syntect_syntax();

		let mut syntax_set_builder = SyntaxSetBuilder::new();
		syntax_set_builder.add(syntax);
		let syntax_set = syntax_set_builder.build();

		TaskmasterHighlighter {
			theme,
			syntax_set
		}
	}

	pub fn highlight(&self, text: &str) -> String {
		let mut h = HighlightLines::new(&self.syntax_set.syntaxes()[0], &self.theme);
		let mut result = String::new();
		
		for line in LinesWithEndings::from(text) {
			let ranges: Vec<(Style, &str)> = h.highlight_line(line, &self.syntax_set).unwrap();
			let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
			result += &escaped;
		}

		result + "\x1b[0m"
	}
}