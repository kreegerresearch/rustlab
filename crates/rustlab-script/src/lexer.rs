use crate::error::ScriptError;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Number(f64),
    Str(String),
    Ident(String),
    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    DotStar,       // .*
    DotSlash,      // ./
    DotCaret,      // .^
    Colon,         // :
    Apostrophe,    // ' (conjugate transpose)
    DotApostrophe, // .' (non-conjugate transpose)
    // Comparison operators
    EqEq,   // ==
    BangEq, // !=
    Lt,     // <
    LtEq,   // <=
    Gt,     // >
    GtEq,   // >=
    // Logical operators
    AmpAmp,   // &&
    PipePipe, // ||
    Bang,     // !
    At,       // @
    // Compound assignment
    PlusEq,  // +=
    MinusEq, // -=
    StarEq,  // *=
    SlashEq, // /=
    // Delimiters
    Eq, // =
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Semicolon,
    // Keywords
    Function,  // function
    End,       // end
    Return,    // return
    If,        // if
    Else,      // else
    For,       // for
    While,     // while
    ElseIf,    // elseif
    Switch,    // switch
    Case,      // case
    Otherwise, // otherwise
    Run,       // run
    Format,    // format
    Hold,      // hold
    Grid,      // grid
    Viewer,    // viewer
    Close,     // close
    Dot,       // . (field access)
    // Structure
    Newline,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Spanned {
    pub token: Token,
    pub line: usize,
}

pub fn tokenize(source: &str) -> Result<Vec<Spanned>, ScriptError> {
    let mut tokens: Vec<Spanned> = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut pos = 0;
    let mut line = 1usize;
    // Track depth inside `[...]` and `{...}` so the whitespace handler below
    // can decide whether a space is a column separator (octave/matlab style)
    // or just decoration outside a literal.
    let mut bracket_depth: i32 = 0;
    let mut brace_depth: i32 = 0;

    while pos < chars.len() {
        let ch = chars[pos];

        match ch {
            ' ' | '\t' | '\r' => {
                // Skip the whitespace run.
                pos += 1;
                while pos < chars.len()
                    && (chars[pos] == ' ' || chars[pos] == '\t' || chars[pos] == '\r')
                {
                    pos += 1;
                }
                // Octave/matlab matrix-literal whitespace rule: inside `[...]`
                // or `{...}`, whitespace between an operand and the start of
                // another operand acts as a column separator (synthetic
                // comma). A leading `+` or `-` immediately followed by an
                // operand (no whitespace after) is treated as unary,
                // marking the start of a new signed element — `[1 -2]` →
                // `[1, -2]`. With whitespace on both sides (`[1 - 2]`) the
                // operator stays binary.
                if (bracket_depth > 0 || brace_depth > 0) && pos < chars.len() {
                    let prev_is_operand = matches!(
                        tokens.last().map(|t| &t.token),
                        Some(Token::Number(_))
                            | Some(Token::Ident(_))
                            | Some(Token::Str(_))
                            | Some(Token::RParen)
                            | Some(Token::RBracket)
                            | Some(Token::RBrace)
                            | Some(Token::Apostrophe)
                            | Some(Token::DotApostrophe)
                            | Some(Token::End)
                    );
                    if prev_is_operand {
                        let nxt = chars[pos];
                        let next_after = chars.get(pos + 1).copied();
                        let starts_operand = nxt.is_ascii_digit()
                            || nxt.is_ascii_alphabetic()
                            || nxt == '_'
                            || nxt == '('
                            || nxt == '['
                            || nxt == '{'
                            || nxt == '"'
                            || (nxt == '.'
                                && next_after.map_or(false, |c| c.is_ascii_digit()));
                        let is_unary_signed = (nxt == '+' || nxt == '-')
                            && next_after.map_or(false, |c| {
                                !matches!(c, ' ' | '\t' | '\r' | '\n' | '=')
                            });
                        if starts_operand || is_unary_signed {
                            tokens.push(Spanned {
                                token: Token::Comma,
                                line,
                            });
                        }
                    }
                }
            }
            '#' | '%' => {
                // Comment: skip until newline (don't consume the newline)
                while pos < chars.len() && chars[pos] != '\n' {
                    pos += 1;
                }
            }
            // Line continuation: ... skips rest of line and the newline
            '.' if pos + 2 < chars.len() && chars[pos + 1] == '.' && chars[pos + 2] == '.' => {
                pos += 3;
                // Skip rest of line (treated as comment)
                while pos < chars.len() && chars[pos] != '\n' {
                    pos += 1;
                }
                // Consume the newline but don't emit a Newline token
                if pos < chars.len() && chars[pos] == '\n' {
                    line += 1;
                    pos += 1;
                }
            }
            '\n' => {
                // Collapse consecutive newlines
                if tokens.last().map(|t| &t.token) != Some(&Token::Newline) {
                    tokens.push(Spanned {
                        token: Token::Newline,
                        line,
                    });
                }
                line += 1;
                pos += 1;
            }
            '+' if pos + 1 < chars.len() && chars[pos + 1] == '=' => {
                tokens.push(Spanned {
                    token: Token::PlusEq,
                    line,
                });
                pos += 2;
            }
            '+' => {
                tokens.push(Spanned {
                    token: Token::Plus,
                    line,
                });
                pos += 1;
            }
            '-' if pos + 1 < chars.len() && chars[pos + 1] == '=' => {
                tokens.push(Spanned {
                    token: Token::MinusEq,
                    line,
                });
                pos += 2;
            }
            '-' => {
                tokens.push(Spanned {
                    token: Token::Minus,
                    line,
                });
                pos += 1;
            }
            '*' if pos + 1 < chars.len() && chars[pos + 1] == '=' => {
                tokens.push(Spanned {
                    token: Token::StarEq,
                    line,
                });
                pos += 2;
            }
            '*' => {
                tokens.push(Spanned {
                    token: Token::Star,
                    line,
                });
                pos += 1;
            }
            '/' if pos + 1 < chars.len() && chars[pos + 1] == '=' => {
                tokens.push(Spanned {
                    token: Token::SlashEq,
                    line,
                });
                pos += 2;
            }
            '/' => {
                tokens.push(Spanned {
                    token: Token::Slash,
                    line,
                });
                pos += 1;
            }
            '^' => {
                tokens.push(Spanned {
                    token: Token::Caret,
                    line,
                });
                pos += 1;
            }
            ':' => {
                tokens.push(Spanned {
                    token: Token::Colon,
                    line,
                });
                pos += 1;
            }
            '\'' => {
                // Context-dependent: transpose after ), ], Ident, Number;
                // otherwise start a single-quoted string literal.
                let is_transpose = matches!(
                    tokens.last().map(|t| &t.token),
                    Some(Token::RParen)
                        | Some(Token::RBracket)
                        | Some(Token::Ident(_))
                        | Some(Token::Number(_))
                        | Some(Token::Apostrophe)
                        | Some(Token::DotApostrophe)
                );
                if is_transpose {
                    tokens.push(Spanned {
                        token: Token::Apostrophe,
                        line,
                    });
                    pos += 1;
                } else {
                    // Single-quoted string literal
                    pos += 1; // skip opening '
                    let start = pos;
                    while pos < chars.len() && chars[pos] != '\'' {
                        if chars[pos] == '\n' {
                            return Err(ScriptError::Lex {
                                line,
                                msg: "unterminated string literal".to_string(),
                            });
                        }
                        pos += 1;
                    }
                    if pos >= chars.len() {
                        return Err(ScriptError::Lex {
                            line,
                            msg: "unterminated string literal".to_string(),
                        });
                    }
                    let s: String = chars[start..pos].iter().collect();
                    tokens.push(Spanned {
                        token: Token::Str(s),
                        line,
                    });
                    pos += 1; // consume closing '
                }
            }
            '=' if pos + 1 < chars.len() && chars[pos + 1] == '=' => {
                tokens.push(Spanned {
                    token: Token::EqEq,
                    line,
                });
                pos += 2;
            }
            '=' => {
                tokens.push(Spanned {
                    token: Token::Eq,
                    line,
                });
                pos += 1;
            }
            '!' if pos + 1 < chars.len() && chars[pos + 1] == '=' => {
                tokens.push(Spanned {
                    token: Token::BangEq,
                    line,
                });
                pos += 2;
            }
            '!' => {
                tokens.push(Spanned {
                    token: Token::Bang,
                    line,
                });
                pos += 1;
            }
            '@' => {
                tokens.push(Spanned {
                    token: Token::At,
                    line,
                });
                pos += 1;
            }
            '<' if pos + 1 < chars.len() && chars[pos + 1] == '=' => {
                tokens.push(Spanned {
                    token: Token::LtEq,
                    line,
                });
                pos += 2;
            }
            '<' => {
                tokens.push(Spanned {
                    token: Token::Lt,
                    line,
                });
                pos += 1;
            }
            '>' if pos + 1 < chars.len() && chars[pos + 1] == '=' => {
                tokens.push(Spanned {
                    token: Token::GtEq,
                    line,
                });
                pos += 2;
            }
            '>' => {
                tokens.push(Spanned {
                    token: Token::Gt,
                    line,
                });
                pos += 1;
            }
            '&' if pos + 1 < chars.len() && chars[pos + 1] == '&' => {
                tokens.push(Spanned {
                    token: Token::AmpAmp,
                    line,
                });
                pos += 2;
            }
            '|' if pos + 1 < chars.len() && chars[pos + 1] == '|' => {
                tokens.push(Spanned {
                    token: Token::PipePipe,
                    line,
                });
                pos += 2;
            }
            '(' => {
                tokens.push(Spanned {
                    token: Token::LParen,
                    line,
                });
                pos += 1;
            }
            ')' => {
                tokens.push(Spanned {
                    token: Token::RParen,
                    line,
                });
                pos += 1;
            }
            '[' => {
                bracket_depth += 1;
                tokens.push(Spanned {
                    token: Token::LBracket,
                    line,
                });
                pos += 1;
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1).max(0);
                tokens.push(Spanned {
                    token: Token::RBracket,
                    line,
                });
                pos += 1;
            }
            '{' => {
                brace_depth += 1;
                tokens.push(Spanned {
                    token: Token::LBrace,
                    line,
                });
                pos += 1;
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1).max(0);
                tokens.push(Spanned {
                    token: Token::RBrace,
                    line,
                });
                pos += 1;
            }
            ',' => {
                tokens.push(Spanned {
                    token: Token::Comma,
                    line,
                });
                pos += 1;
            }
            ';' => {
                tokens.push(Spanned {
                    token: Token::Semicolon,
                    line,
                });
                pos += 1;
            }
            // Dot operators (.*  ./  .^  .') must be checked before the number branch
            '.' if pos + 1 < chars.len() && chars[pos + 1] == '*' => {
                tokens.push(Spanned {
                    token: Token::DotStar,
                    line,
                });
                pos += 2;
            }
            '.' if pos + 1 < chars.len() && chars[pos + 1] == '/' => {
                tokens.push(Spanned {
                    token: Token::DotSlash,
                    line,
                });
                pos += 2;
            }
            '.' if pos + 1 < chars.len() && chars[pos + 1] == '^' => {
                tokens.push(Spanned {
                    token: Token::DotCaret,
                    line,
                });
                pos += 2;
            }
            '.' if pos + 1 < chars.len() && chars[pos + 1] == '\'' => {
                tokens.push(Spanned {
                    token: Token::DotApostrophe,
                    line,
                });
                pos += 2;
            }
            // Field access: . followed by an identifier character
            '.' if pos + 1 < chars.len()
                && (chars[pos + 1].is_alphabetic() || chars[pos + 1] == '_') =>
            {
                tokens.push(Spanned {
                    token: Token::Dot,
                    line,
                });
                pos += 1;
            }
            '"' => {
                // String literal
                pos += 1;
                let start = pos;
                while pos < chars.len() && chars[pos] != '"' {
                    if chars[pos] == '\n' {
                        return Err(ScriptError::Lex {
                            line,
                            msg: "unterminated string literal".to_string(),
                        });
                    }
                    pos += 1;
                }
                if pos >= chars.len() {
                    return Err(ScriptError::Lex {
                        line,
                        msg: "unterminated string literal".to_string(),
                    });
                }
                let s: String = chars[start..pos].iter().collect();
                tokens.push(Spanned {
                    token: Token::Str(s),
                    line,
                });
                pos += 1; // consume closing "
            }
            c if c.is_ascii_digit() || c == '.' => {
                // Number — underscores allowed as digit separators (e.g. 1_000_000)
                let start = pos;
                while pos < chars.len()
                    && (chars[pos].is_ascii_digit() || chars[pos] == '.' || chars[pos] == '_')
                {
                    pos += 1;
                }
                // Optional exponent: e or E, optional sign
                if pos < chars.len() && (chars[pos] == 'e' || chars[pos] == 'E') {
                    pos += 1;
                    if pos < chars.len() && (chars[pos] == '+' || chars[pos] == '-') {
                        pos += 1;
                    }
                    while pos < chars.len() && (chars[pos].is_ascii_digit() || chars[pos] == '_') {
                        pos += 1;
                    }
                }
                // Strip underscores before parsing
                let num_str: String = chars[start..pos].iter().filter(|c| **c != '_').collect();
                let val: f64 = num_str.parse().map_err(|_| ScriptError::Lex {
                    line,
                    msg: format!("invalid number: {}", num_str),
                })?;
                tokens.push(Spanned {
                    token: Token::Number(val),
                    line,
                });
            }
            c if c.is_alphabetic() || c == '_' => {
                // Identifier or keyword
                let start = pos;
                while pos < chars.len() && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
                    pos += 1;
                }
                let ident: String = chars[start..pos].iter().collect();
                let tok = match ident.as_str() {
                    "function" => Token::Function,
                    "end" => Token::End,
                    "return" => Token::Return,
                    "if" => Token::If,
                    "elseif" => Token::ElseIf,
                    "else" => Token::Else,
                    "for" => Token::For,
                    "while" => Token::While,
                    "switch" => Token::Switch,
                    "case" => Token::Case,
                    "otherwise" => Token::Otherwise,
                    "run" => Token::Run,
                    "format" => Token::Format,
                    "hold" => Token::Hold,
                    "grid" => Token::Grid,
                    "viewer" => Token::Viewer,
                    "close" => Token::Close,
                    _ => Token::Ident(ident),
                };
                tokens.push(Spanned { token: tok, line });
            }
            other => {
                return Err(ScriptError::Lex {
                    line,
                    msg: format!("unexpected character: {:?}", other),
                });
            }
        }
    }

    tokens.push(Spanned {
        token: Token::Eof,
        line,
    });
    Ok(tokens)
}
