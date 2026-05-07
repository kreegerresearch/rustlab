use crate::ast::{BinOp, Expr, Stmt, StmtKind};
use crate::error::ScriptError;
use crate::lexer::{Spanned, Token};

pub fn parse(tokens: Vec<Spanned>) -> Result<Vec<Stmt>, ScriptError> {
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

/// Terminator tells `parse_block_body` what caused it to stop.
enum BlockEnd {
    End,
    Else,
    ElseIf,
}

struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Spanned>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn current(&self) -> &Spanned {
        &self.tokens[self.pos]
    }

    fn current_line(&self) -> usize {
        self.current().line
    }

    fn peek_token(&self) -> &Token {
        &self.current().token
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos].token;
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ScriptError> {
        if self.peek_token() == expected {
            self.advance();
            Ok(())
        } else {
            Err(ScriptError::Parse {
                line: self.current_line(),
                msg: format!("expected {:?}, got {:?}", expected, self.peek_token()),
            })
        }
    }

    /// Skip newlines (used inside delimiters like [...] and (...))
    fn skip_newlines(&mut self) {
        while self.peek_token() == &Token::Newline {
            self.advance();
        }
    }

    fn parse_program(&mut self) -> Result<Vec<Stmt>, ScriptError> {
        self.parse_stmts_until_end(false)
    }

    /// Parse statements until EOF (top-level) or `end` keyword (function body).
    /// `inside_fn` = true means we stop at `Token::End` or `Token::Eof`.
    fn parse_stmts_until_end(&mut self, inside_fn: bool) -> Result<Vec<Stmt>, ScriptError> {
        let mut stmts = Vec::new();
        loop {
            match self.peek_token() {
                Token::Eof => {
                    if inside_fn {
                        return Err(ScriptError::Parse {
                            line: self.current_line(),
                            msg: "unexpected end of input: missing 'end' for function".to_string(),
                        });
                    }
                    break;
                }
                Token::End => {
                    if inside_fn {
                        self.advance(); // consume 'end'
                                        // consume optional ';' and newline
                        if self.peek_token() == &Token::Semicolon {
                            self.advance();
                        }
                        if self.peek_token() == &Token::Newline {
                            self.advance();
                        }
                        break;
                    } else {
                        return Err(ScriptError::Parse {
                            line: self.current_line(),
                            msg: "unexpected 'end' outside of function".to_string(),
                        });
                    }
                }
                Token::Newline => {
                    self.advance();
                }
                Token::Function => {
                    stmts.push(self.parse_function_def()?);
                }
                Token::Return => {
                    let line = self.current_line();
                    self.advance();
                    let suppress = self.consume_stmt_end()?;
                    let _ = suppress;
                    stmts.push(Stmt::new(StmtKind::Return, line));
                }
                Token::If => {
                    stmts.push(self.parse_if_stmt()?);
                }
                Token::For => {
                    stmts.push(self.parse_for_stmt()?);
                }
                Token::While => {
                    stmts.push(self.parse_while_stmt()?);
                }
                Token::Switch => {
                    stmts.push(self.parse_switch_stmt()?);
                }
                Token::Run => {
                    stmts.push(self.parse_run_stmt()?);
                }
                Token::Format => {
                    stmts.push(self.parse_format_stmt()?);
                }
                Token::Hold => {
                    stmts.push(self.parse_on_off_stmt("hold")?);
                }
                Token::Grid => {
                    stmts.push(self.parse_on_off_stmt("grid")?);
                }
                Token::Viewer => {
                    stmts.push(self.parse_on_off_stmt("viewer")?);
                }
                Token::Close => {
                    stmts.push(self.parse_close_stmt()?);
                }
                Token::Else | Token::ElseIf => {
                    return Err(ScriptError::Parse {
                        line: self.current_line(),
                        msg: format!("unexpected '{:?}' without matching 'if'", self.peek_token()),
                    });
                }
                Token::LBracket if self.is_multi_assign() => {
                    stmts.push(self.parse_multi_assign()?);
                }
                Token::Ident(_) => {
                    let stmt = if self.is_field_assignment() {
                        self.parse_field_assignment()?
                    } else if self.is_index_assignment() {
                        self.parse_index_assign()?
                    } else if self.is_assignment() {
                        self.parse_assignment()?
                    } else {
                        self.parse_expr_stmt()?
                    };
                    stmts.push(stmt);
                }
                _ => {
                    stmts.push(self.parse_expr_stmt()?);
                }
            }
        }
        Ok(stmts)
    }

    /// Peek ahead to decide if we have `IDENT ( ... ) =` (not `==`) — indexed assignment.
    /// Uses paren-depth counting to find the matching `)`.
    fn is_index_assignment(&self) -> bool {
        if !matches!(self.peek_token(), Token::Ident(_)) {
            return false;
        }
        if !matches!(
            self.tokens.get(self.pos + 1).map(|s| &s.token),
            Some(Token::LParen)
        ) {
            return false;
        }
        let mut depth = 0usize;
        let mut p = self.pos + 1;
        while p < self.tokens.len() {
            match &self.tokens[p].token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(self.tokens.get(p + 1).map(|s| &s.token), Some(Token::Eq))
                            && !matches!(
                                self.tokens.get(p + 2).map(|s| &s.token),
                                Some(Token::Eq)
                            );
                    }
                }
                Token::Newline | Token::Eof => break,
                _ => {}
            }
            p += 1;
        }
        false
    }

    fn parse_index_assign(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        let name = match self.advance() {
            Token::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        self.advance(); // consume '('
        self.skip_newlines();
        let indices = self.parse_arglist()?;
        self.skip_newlines();
        self.expect(&Token::RParen)?;
        self.advance(); // consume '='
        let expr = self.parse_range_expr()?;
        let suppress = self.consume_stmt_end()?;
        Ok(Stmt::new(
            StmtKind::IndexAssign {
                name,
                indices,
                expr,
                suppress,
            },
            line,
        ))
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        self.advance(); // consume 'while'
        let cond = self.parse_range_expr()?;
        let _ = self.consume_stmt_end()?;
        let body = self.parse_stmts_until_end(true)?;
        Ok(Stmt::new(StmtKind::While { cond, body }, line))
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        self.advance(); // consume 'for'
        let var = match self.peek_token().clone() {
            Token::Ident(s) => {
                self.advance();
                s
            }
            other => {
                return Err(ScriptError::Parse {
                    line: self.current_line(),
                    msg: format!("expected loop variable after 'for', got {:?}", other),
                })
            }
        };
        self.expect(&Token::Eq)?;
        let iter = self.parse_range_expr()?;
        let _ = self.consume_stmt_end()?;
        let body = self.parse_stmts_until_end(true)?;
        Ok(Stmt::new(StmtKind::For { var, iter, body }, line))
    }

    /// Peek ahead to decide if we have `IDENT = expr` or `IDENT += expr` etc.
    fn is_assignment(&self) -> bool {
        if self.pos + 1 < self.tokens.len() {
            // Plain assignment: `=` but not `==`
            let is_plain = matches!(self.tokens[self.pos + 1].token, Token::Eq)
                && !matches!(
                    self.tokens.get(self.pos + 2).map(|s| &s.token),
                    Some(Token::Eq)
                );
            // Compound assignment: +=, -=, *=, /=
            let is_compound = matches!(
                self.tokens[self.pos + 1].token,
                Token::PlusEq | Token::MinusEq | Token::StarEq | Token::SlashEq
            );
            is_plain || is_compound
        } else {
            false
        }
    }

    /// Peek ahead to decide if we have `IDENT . IDENT = expr` (struct field assignment)
    fn is_field_assignment(&self) -> bool {
        self.pos + 3 < self.tokens.len()
            && matches!(self.tokens[self.pos].token, Token::Ident(_))
            && self.tokens[self.pos + 1].token == Token::Dot
            && matches!(self.tokens[self.pos + 2].token, Token::Ident(_))
            && self.tokens[self.pos + 3].token == Token::Eq
            && !matches!(
                self.tokens.get(self.pos + 4).map(|s| &s.token),
                Some(Token::Eq)
            )
    }

    fn parse_field_assignment(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        let object = match self.advance() {
            Token::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        self.advance(); // consume '.'
        let field = match self.advance() {
            Token::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        self.advance(); // consume '='
        let expr = self.parse_range_expr()?;
        let suppress = self.consume_stmt_end()?;
        Ok(Stmt::new(
            StmtKind::FieldAssign {
                object,
                field,
                expr,
                suppress,
            },
            line,
        ))
    }

    fn parse_function_def(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        self.advance(); // consume 'function'
        self.skip_newlines();

        // Three signature shapes:
        //   function name(params) ... end                  → 0 outputs
        //   function retvar = name(params) ... end         → 1 output
        //   function [a, b, ...] = name(params) ... end    → N outputs (matlab)
        let (return_vars, name) = if self.pos < self.tokens.len()
            && self.tokens[self.pos].token == Token::LBracket
        {
            // [a, b, ...] = name(params) — multi-output form.
            self.advance(); // consume '['
            let mut names: Vec<String> = Vec::new();
            loop {
                match self.peek_token().clone() {
                    Token::Ident(s) => {
                        self.advance();
                        names.push(s);
                    }
                    other => {
                        return Err(ScriptError::Parse {
                            line: self.current_line(),
                            msg: format!(
                                "expected identifier in function output list, got {:?}",
                                other
                            ),
                        })
                    }
                }
                if self.peek_token() == &Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&Token::RBracket)?;
            if names.is_empty() {
                return Err(ScriptError::Parse {
                    line: self.current_line(),
                    msg: "function output list cannot be empty — write `function name(...)` for no return"
                        .to_string(),
                });
            }
            self.expect(&Token::Eq)?;
            let line_now = self.current_line();
            let n = match self.advance() {
                Token::Ident(s) => s.clone(),
                other => {
                    return Err(ScriptError::Parse {
                        line: line_now,
                        msg: format!("expected function name after '=', got {:?}", other),
                    })
                }
            };
            (names, n)
        } else if self.pos + 3 < self.tokens.len()
            && matches!(self.tokens[self.pos].token, Token::Ident(_))
            && self.tokens[self.pos + 1].token == Token::Eq
            && matches!(self.tokens[self.pos + 2].token, Token::Ident(_))
            && self.tokens[self.pos + 3].token == Token::LParen
        {
            // retvar = name(params) — single-output form.
            let ret = match self.advance() {
                Token::Ident(s) => s.clone(),
                _ => unreachable!(),
            };
            self.advance(); // consume '='
            let n = match self.advance() {
                Token::Ident(s) => s.clone(),
                _ => unreachable!(),
            };
            (vec![ret], n)
        } else {
            // No-output form: name(params).
            let n = match self.peek_token().clone() {
                Token::Ident(s) => {
                    self.advance();
                    s
                }
                other => {
                    return Err(ScriptError::Parse {
                        line: self.current_line(),
                        msg: format!("expected function name, got {:?}", other),
                    })
                }
            };
            (Vec::new(), n)
        };

        // Parameter list
        self.expect(&Token::LParen)?;
        let params = if self.peek_token() == &Token::RParen {
            vec![]
        } else {
            self.parse_param_list()?
        };
        self.expect(&Token::RParen)?;
        let _ = self.consume_stmt_end()?;

        // Body — parsed until `end`
        let body = self.parse_stmts_until_end(true)?;

        Ok(Stmt::new(
            StmtKind::FunctionDef {
                name,
                params,
                return_vars,
                body,
            },
            line,
        ))
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        self.advance(); // consume 'if'
        let cond = self.parse_range_expr()?;
        let _ = self.consume_stmt_end()?;

        let (then_body, term) = self.parse_block_body("if")?;

        // Collect elseif arms
        let mut elseif_arms: Vec<(Expr, Vec<Stmt>)> = Vec::new();
        let mut terminator = term;
        while matches!(terminator, BlockEnd::ElseIf) {
            let ei_cond = self.parse_range_expr()?;
            let _ = self.consume_stmt_end()?;
            let (ei_body, t) = self.parse_block_body("elseif")?;
            elseif_arms.push((ei_cond, ei_body));
            terminator = t;
        }

        // Parse else-body if present
        let else_body = if matches!(terminator, BlockEnd::Else) {
            let (body, _) = self.parse_block_body("else")?;
            body
        } else {
            vec![]
        };

        Ok(Stmt::new(
            StmtKind::If {
                cond,
                then_body,
                elseif_arms,
                else_body,
            },
            line,
        ))
    }

    /// Parse statements until `end`, `else`, or `elseif`.
    /// Consumes the terminating keyword. Returns (body, what_terminated_it).
    fn parse_block_body(&mut self, context: &str) -> Result<(Vec<Stmt>, BlockEnd), ScriptError> {
        let mut body = Vec::new();
        loop {
            match self.peek_token() {
                Token::Eof => {
                    return Err(ScriptError::Parse {
                        line: self.current_line(),
                        msg: format!("unexpected end of input: missing 'end' for '{}'", context),
                    });
                }
                Token::End => {
                    self.advance();
                    if self.peek_token() == &Token::Semicolon {
                        self.advance();
                    }
                    if self.peek_token() == &Token::Newline {
                        self.advance();
                    }
                    return Ok((body, BlockEnd::End));
                }
                Token::Else => {
                    self.advance();
                    if self.peek_token() == &Token::Semicolon {
                        self.advance();
                    }
                    if self.peek_token() == &Token::Newline {
                        self.advance();
                    }
                    return Ok((body, BlockEnd::Else));
                }
                Token::ElseIf => {
                    self.advance(); // consume 'elseif'
                    return Ok((body, BlockEnd::ElseIf));
                }
                _ => {
                    body.push(self.parse_one_body_stmt()?);
                }
            }
        }
    }

    /// Parse a single statement inside a block body (if/else/elseif/for/while/switch).
    fn parse_one_body_stmt(&mut self) -> Result<Stmt, ScriptError> {
        match self.peek_token() {
            Token::Newline => {
                self.advance();
                self.parse_one_body_stmt()
            }
            Token::Function => self.parse_function_def(),
            Token::Return => {
                let line = self.current_line();
                self.advance();
                let _ = self.consume_stmt_end()?;
                Ok(Stmt::new(StmtKind::Return, line))
            }
            Token::If => self.parse_if_stmt(),
            Token::For => self.parse_for_stmt(),
            Token::While => self.parse_while_stmt(),
            Token::Switch => self.parse_switch_stmt(),
            Token::Run => self.parse_run_stmt(),
            Token::Format => self.parse_format_stmt(),
            Token::Hold => self.parse_on_off_stmt("hold"),
            Token::Grid => self.parse_on_off_stmt("grid"),
            Token::Viewer => self.parse_on_off_stmt("viewer"),
            Token::Close => self.parse_close_stmt(),
            Token::LBracket if self.is_multi_assign() => self.parse_multi_assign(),
            Token::Ident(_) => {
                if self.is_field_assignment() {
                    self.parse_field_assignment()
                } else if self.is_index_assignment() {
                    self.parse_index_assign()
                } else if self.is_assignment() {
                    self.parse_assignment()
                } else {
                    self.parse_expr_stmt()
                }
            }
            _ => self.parse_expr_stmt(),
        }
    }

    fn parse_switch_stmt(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        self.advance(); // consume 'switch'
        let expr = self.parse_range_expr()?;
        let _ = self.consume_stmt_end()?;

        let mut cases: Vec<(Expr, Vec<Stmt>)> = Vec::new();
        let mut otherwise: Vec<Stmt> = Vec::new();

        // Skip leading newlines
        while self.peek_token() == &Token::Newline {
            self.advance();
        }

        loop {
            match self.peek_token() {
                Token::Case => {
                    self.advance(); // consume 'case'
                    let case_val = self.parse_range_expr()?;
                    let _ = self.consume_stmt_end()?;
                    let mut body = Vec::new();
                    loop {
                        match self.peek_token() {
                            Token::Case | Token::Otherwise | Token::End | Token::Eof => break,
                            Token::Newline => {
                                self.advance();
                            }
                            _ => {
                                body.push(self.parse_one_body_stmt()?);
                            }
                        }
                    }
                    cases.push((case_val, body));
                }
                Token::Otherwise => {
                    self.advance(); // consume 'otherwise'
                    if self.peek_token() == &Token::Newline {
                        self.advance();
                    }
                    loop {
                        match self.peek_token() {
                            Token::End | Token::Eof => break,
                            Token::Newline => {
                                self.advance();
                            }
                            _ => {
                                otherwise.push(self.parse_one_body_stmt()?);
                            }
                        }
                    }
                }
                Token::End => {
                    self.advance();
                    if self.peek_token() == &Token::Semicolon {
                        self.advance();
                    }
                    if self.peek_token() == &Token::Newline {
                        self.advance();
                    }
                    break;
                }
                Token::Eof => {
                    return Err(ScriptError::Parse {
                        line: self.current_line(),
                        msg: "unexpected end of input: missing 'end' for 'switch'".to_string(),
                    });
                }
                Token::Newline => {
                    self.advance();
                }
                other => {
                    return Err(ScriptError::Parse {
                        line: self.current_line(),
                        msg: format!(
                            "expected 'case', 'otherwise', or 'end' in switch, got {:?}",
                            other
                        ),
                    });
                }
            }
        }

        Ok(Stmt::new(
            StmtKind::Switch {
                expr,
                cases,
                otherwise,
            },
            line,
        ))
    }

    fn parse_run_stmt(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        self.advance(); // consume 'run'
                        // Collect the rest of the line as a file path (unquoted)
        let mut path_chars = Vec::new();
        loop {
            match self.peek_token() {
                Token::Newline | Token::Eof | Token::Semicolon => break,
                _ => {
                    // Reconstruct path from tokens
                    let tok = self.advance().clone();
                    match &tok {
                        Token::Ident(s) => path_chars.push(s.clone()),
                        Token::Dot => path_chars.push(".".to_string()),
                        Token::Slash => path_chars.push("/".to_string()),
                        Token::Minus => path_chars.push("-".to_string()),
                        Token::Number(n) => path_chars.push(format!("{}", n)),
                        Token::Str(s) => path_chars.push(s.clone()),
                        other => path_chars.push(format!("{:?}", other)),
                    }
                }
            }
        }
        let _ = self.consume_stmt_end()?;
        let path = path_chars.join("").trim().to_string();
        if path.is_empty() {
            return Err(ScriptError::Parse {
                line: self.current_line(),
                msg: "run: expected a file path".to_string(),
            });
        }
        Ok(Stmt::new(StmtKind::Run { path }, line))
    }

    /// Parse `hold on` / `hold off` / `grid on` / `grid off` (bare command)
    /// or function-call form: `hold("on")`, `grid(1)`.
    fn parse_on_off_stmt(&mut self, cmd: &str) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        self.advance(); // consume keyword

        // Bare `viewer` (no on/off, no args) — status query.
        if cmd == "viewer"
            && matches!(
                self.peek_token(),
                Token::Newline | Token::Eof | Token::Semicolon
            )
        {
            let _ = self.consume_stmt_end()?;
            return Ok(Stmt::new(
                StmtKind::Viewer {
                    on: None,
                    name: None,
                },
                line,
            ));
        }

        // Bare form: `hold on` / `grid off` / `viewer on <name>`
        if let Token::Ident(s) = self.peek_token() {
            let val = match s.as_str() {
                "on" => true,
                "off" => false,
                other => {
                    return Err(ScriptError::Parse {
                        line: self.current_line(),
                        msg: format!("{}: expected 'on' or 'off', got '{}'", cmd, other),
                    })
                }
            };
            self.advance();
            // For `viewer on`, optionally read a session name
            let viewer_name = if cmd == "viewer" && val {
                if let Token::Ident(name) = self.peek_token() {
                    let name = name.clone();
                    self.advance();
                    Some(name)
                } else {
                    None
                }
            } else {
                None
            };
            let _ = self.consume_stmt_end()?;
            return match cmd {
                "hold" => Ok(Stmt::new(StmtKind::Hold { on: val }, line)),
                "grid" => Ok(Stmt::new(StmtKind::Grid { on: val }, line)),
                "viewer" => Ok(Stmt::new(
                    StmtKind::Viewer {
                        on: Some(val),
                        name: viewer_name,
                    },
                    line,
                )),
                _ => unreachable!(),
            };
        }

        // Function-call form: `hold("on")` / `grid(0)`
        // Desugar to Expr::Call so the existing builtins handle it.
        if matches!(self.peek_token(), Token::LParen) {
            self.advance(); // consume '('
            let arg = self.parse_range_expr()?;
            self.expect(&Token::RParen)?;
            let suppress = self.consume_stmt_end()?;
            let call = Expr::Call {
                name: cmd.to_string(),
                args: vec![arg],
            };
            return Ok(Stmt::new(StmtKind::Expr(call, suppress), line));
        }

        Err(ScriptError::Parse {
            line: self.current_line(),
            msg: format!(
                "{}: expected 'on', 'off', or '(' — got {:?}",
                cmd,
                self.peek_token()
            ),
        })
    }

    /// Parse `close`, `close all`, `close N`, or `close(...)`. All forms
    /// desugar to a regular builtin call so the AST stays small.
    ///   close            → close()
    ///   close all        → close("all")
    ///   close N          → close(N)
    ///   close(args...)   → close(args...)   (function-call form)
    fn parse_close_stmt(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        self.advance(); // consume `close` keyword

        // Bare `close` — close current figure.
        if matches!(
            self.peek_token(),
            Token::Newline | Token::Eof | Token::Semicolon
        ) {
            let suppress = self.consume_stmt_end()?;
            let call = Expr::Call {
                name: "close".to_string(),
                args: vec![],
            };
            return Ok(Stmt::new(StmtKind::Expr(call, suppress), line));
        }

        // `close all` bareword — close every figure.
        if let Token::Ident(s) = self.peek_token() {
            if s == "all" {
                self.advance();
                let suppress = self.consume_stmt_end()?;
                let call = Expr::Call {
                    name: "close".to_string(),
                    args: vec![Expr::Str("all".to_string())],
                };
                return Ok(Stmt::new(StmtKind::Expr(call, suppress), line));
            }
        }

        // `close(args...)` — function-call form.
        if matches!(self.peek_token(), Token::LParen) {
            self.advance(); // consume '('
            let mut args = Vec::new();
            if !matches!(self.peek_token(), Token::RParen) {
                args.push(self.parse_range_expr()?);
                while matches!(self.peek_token(), Token::Comma) {
                    self.advance();
                    args.push(self.parse_range_expr()?);
                }
            }
            self.expect(&Token::RParen)?;
            let suppress = self.consume_stmt_end()?;
            let call = Expr::Call {
                name: "close".to_string(),
                args,
            };
            return Ok(Stmt::new(StmtKind::Expr(call, suppress), line));
        }

        // `close N` (bareword arg) — desugar to close(N).
        let arg = self.parse_range_expr()?;
        let suppress = self.consume_stmt_end()?;
        let call = Expr::Call {
            name: "close".to_string(),
            args: vec![arg],
        };
        Ok(Stmt::new(StmtKind::Expr(call, suppress), line))
    }

    fn parse_format_stmt(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        self.advance(); // consume 'format'
        let mode = match self.peek_token() {
            Token::Ident(s) => {
                let m = s.clone();
                self.advance();
                m
            }
            Token::Newline | Token::Eof | Token::Semicolon => {
                // bare `format` with no arg — show current mode
                String::new()
            }
            other => {
                return Err(ScriptError::Parse {
                    line: self.current_line(),
                    msg: format!(
                        "format: expected mode name (commas, default), got {:?}",
                        other
                    ),
                });
            }
        };
        let _ = self.consume_stmt_end()?;
        Ok(Stmt::new(StmtKind::Format { mode }, line))
    }

    /// `[IDENT, IDENT, ...] =` (not `==`) at statement level
    fn is_multi_assign(&self) -> bool {
        if !matches!(self.peek_token(), Token::LBracket) {
            return false;
        }
        let mut p = self.pos + 1;
        // Expect at least one IDENT
        if !matches!(self.tokens.get(p).map(|s| &s.token), Some(Token::Ident(_))) {
            return false;
        }
        p += 1;
        // Optional , IDENT pairs
        loop {
            match self.tokens.get(p).map(|s| &s.token) {
                Some(Token::Comma) => {
                    p += 1;
                    if !matches!(self.tokens.get(p).map(|s| &s.token), Some(Token::Ident(_))) {
                        return false;
                    }
                    p += 1;
                }
                Some(Token::RBracket) => {
                    p += 1;
                    break;
                }
                _ => return false,
            }
        }
        matches!(self.tokens.get(p).map(|s| &s.token), Some(Token::Eq))
            && !matches!(self.tokens.get(p + 1).map(|s| &s.token), Some(Token::Eq))
    }

    fn parse_multi_assign(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        self.advance(); // consume '['
        let mut names = Vec::new();
        names.push(match self.peek_token().clone() {
            Token::Ident(s) => {
                self.advance();
                s
            }
            _ => unreachable!(),
        });
        while self.peek_token() == &Token::Comma {
            self.advance();
            names.push(match self.peek_token().clone() {
                Token::Ident(s) => {
                    self.advance();
                    s
                }
                other => {
                    return Err(ScriptError::Parse {
                        line: self.current_line(),
                        msg: format!("expected name in multi-assign list, got {:?}", other),
                    })
                }
            });
        }
        self.expect(&Token::RBracket)?;
        self.advance(); // consume '='
        let expr = self.parse_range_expr()?;
        let suppress = self.consume_stmt_end()?;
        Ok(Stmt::new(
            StmtKind::MultiAssign {
                names,
                expr,
                suppress,
            },
            line,
        ))
    }

    fn parse_param_list(&mut self) -> Result<Vec<String>, ScriptError> {
        let mut params = Vec::new();
        params.push(match self.peek_token().clone() {
            Token::Ident(s) => {
                self.advance();
                s
            }
            other => {
                return Err(ScriptError::Parse {
                    line: self.current_line(),
                    msg: format!("expected parameter name, got {:?}", other),
                })
            }
        });
        while self.peek_token() == &Token::Comma {
            self.advance();
            params.push(match self.peek_token().clone() {
                Token::Ident(s) => {
                    self.advance();
                    s
                }
                other => {
                    return Err(ScriptError::Parse {
                        line: self.current_line(),
                        msg: format!("expected parameter name, got {:?}", other),
                    })
                }
            });
        }
        Ok(params)
    }

    fn parse_assignment(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        let name = match self.advance() {
            Token::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        // Check for compound assignment (+=, -=, *=, /=) or plain =
        let compound_op = match self.peek_token() {
            Token::PlusEq => {
                self.advance();
                Some(BinOp::Add)
            }
            Token::MinusEq => {
                self.advance();
                Some(BinOp::Sub)
            }
            Token::StarEq => {
                self.advance();
                Some(BinOp::Mul)
            }
            Token::SlashEq => {
                self.advance();
                Some(BinOp::Div)
            }
            _ => {
                self.advance();
                None
            } // plain '='
        };
        let rhs = self.parse_range_expr()?;
        let expr = match compound_op {
            Some(op) => Expr::BinOp {
                op,
                lhs: Box::new(Expr::Var(name.clone())),
                rhs: Box::new(rhs),
            },
            None => rhs,
        };
        let suppress = self.consume_stmt_end()?;
        Ok(Stmt::new(
            StmtKind::Assign {
                name,
                expr,
                suppress,
            },
            line,
        ))
    }

    fn parse_expr_stmt(&mut self) -> Result<Stmt, ScriptError> {
        let line = self.current_line();
        let expr = self.parse_range_expr()?;
        let suppress = self.consume_stmt_end()?;
        Ok(Stmt::new(StmtKind::Expr(expr, suppress), line))
    }

    /// range_expr = logical_or (":" logical_or (":" logical_or)?)?
    /// Handles `start:stop` and `start:step:stop` range syntax.
    fn parse_range_expr(&mut self) -> Result<Expr, ScriptError> {
        let first = self.parse_logical_or()?;
        if self.peek_token() == &Token::Colon {
            self.advance(); // consume ':'
            let second = self.parse_expr()?;
            if self.peek_token() == &Token::Colon {
                self.advance(); // consume second ':'
                let third = self.parse_logical_or()?;
                // start:step:stop
                Ok(Expr::Range {
                    start: Box::new(first),
                    step: Some(Box::new(second)),
                    stop: Box::new(third),
                })
            } else {
                // start:stop  (step defaults to 1)
                Ok(Expr::Range {
                    start: Box::new(first),
                    step: None,
                    stop: Box::new(second),
                })
            }
        } else {
            Ok(first)
        }
    }

    /// Consume an optional trailing `;` then a newline or EOF.
    /// Returns true (suppress output) if a `;` was present.
    fn consume_stmt_end(&mut self) -> Result<bool, ScriptError> {
        let suppress = if self.peek_token() == &Token::Semicolon {
            self.advance();
            true
        } else {
            false
        };
        match self.peek_token() {
            Token::Newline => {
                self.advance();
                Ok(suppress)
            }
            Token::Eof => Ok(suppress),
            // Comma acts as a statement separator (e.g. single-line if: `if cond, body; end`)
            Token::Comma => {
                self.advance();
                Ok(suppress)
            }
            // Allow implicit end when the next token is a keyword that terminates a block
            Token::End | Token::Else | Token::ElseIf | Token::Case | Token::Otherwise => {
                Ok(suppress)
            }
            // Semicolon already consumed → next token starts a new statement on same line
            _ if suppress => Ok(suppress),
            other => Err(ScriptError::Parse {
                line: self.current_line(),
                msg: format!("expected newline or EOF, got {:?}", other),
            }),
        }
    }

    // logical_or = logical_and ('||' logical_and)*
    fn parse_logical_or(&mut self) -> Result<Expr, ScriptError> {
        let mut lhs = self.parse_logical_and()?;
        while self.peek_token() == &Token::PipePipe {
            self.advance();
            let rhs = self.parse_logical_and()?;
            lhs = Expr::BinOp {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    // logical_and = comparison ('&&' comparison)*
    fn parse_logical_and(&mut self) -> Result<Expr, ScriptError> {
        let mut lhs = self.parse_comparison()?;
        while self.peek_token() == &Token::AmpAmp {
            self.advance();
            let rhs = self.parse_comparison()?;
            lhs = Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    // comparison = expr (('==' | '!=' | '<' | '<=' | '>' | '>=') expr)?
    fn parse_comparison(&mut self) -> Result<Expr, ScriptError> {
        let lhs = self.parse_expr()?;
        let op = match self.peek_token() {
            Token::EqEq => BinOp::Eq,
            Token::BangEq => BinOp::Ne,
            Token::Lt => BinOp::Lt,
            Token::LtEq => BinOp::Le,
            Token::Gt => BinOp::Gt,
            Token::GtEq => BinOp::Ge,
            _ => return Ok(lhs),
        };
        self.advance();
        let rhs = self.parse_expr()?;
        Ok(Expr::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }

    // expr = term (('+' | '-') term)*
    fn parse_expr(&mut self) -> Result<Expr, ScriptError> {
        let mut lhs = self.parse_term()?;
        loop {
            match self.peek_token() {
                Token::Plus => {
                    self.advance();
                    let rhs = self.parse_term()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Add,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                Token::Minus => {
                    self.advance();
                    let rhs = self.parse_term()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Sub,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    // term = unary (('*' | '/' | '.*' | './') unary)*
    fn parse_term(&mut self) -> Result<Expr, ScriptError> {
        let mut lhs = self.parse_unary()?;
        loop {
            match self.peek_token() {
                Token::Star => {
                    self.advance();
                    let r = self.parse_unary()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Mul,
                        lhs: Box::new(lhs),
                        rhs: Box::new(r),
                    };
                }
                Token::Slash => {
                    self.advance();
                    let r = self.parse_unary()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Div,
                        lhs: Box::new(lhs),
                        rhs: Box::new(r),
                    };
                }
                Token::DotStar => {
                    self.advance();
                    let r = self.parse_unary()?;
                    lhs = Expr::BinOp {
                        op: BinOp::ElemMul,
                        lhs: Box::new(lhs),
                        rhs: Box::new(r),
                    };
                }
                Token::DotSlash => {
                    self.advance();
                    let r = self.parse_unary()?;
                    lhs = Expr::BinOp {
                        op: BinOp::ElemDiv,
                        lhs: Box::new(lhs),
                        rhs: Box::new(r),
                    };
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    // unary = ('+' | '-' | '!') unary | factor
    //
    // Unary minus/not sits BELOW power (`^`, `.^`) so `-x.^2` parses as
    // `-(x.^2)` — matching Octave precedence. The RHS of `^`/`.^`
    // also goes through unary so `2^-3` still parses as `2^(-3)`.
    // Unary `+` is a no-op pass-through; it exists so octave-style
    // matrix literals like `[1 +2]` work after the lexer's
    // whitespace-as-separator rule.
    fn parse_unary(&mut self) -> Result<Expr, ScriptError> {
        match self.peek_token() {
            Token::Plus => {
                self.advance();
                self.parse_unary()
            }
            Token::Minus => {
                self.advance();
                let inner = self.parse_unary()?;
                Ok(Expr::UnaryMinus(Box::new(inner)))
            }
            Token::Bang => {
                self.advance();
                let inner = self.parse_unary()?;
                Ok(Expr::UnaryNot(Box::new(inner)))
            }
            _ => self.parse_factor(),
        }
    }

    // factor = postfix ('^' | '.^' unary)?   right-associative
    fn parse_factor(&mut self) -> Result<Expr, ScriptError> {
        let base = self.parse_postfix()?;
        match self.peek_token() {
            Token::Caret => {
                self.advance();
                let exp = self.parse_unary()?;
                Ok(Expr::BinOp {
                    op: BinOp::Pow,
                    lhs: Box::new(base),
                    rhs: Box::new(exp),
                })
            }
            Token::DotCaret => {
                self.advance();
                let exp = self.parse_unary()?;
                Ok(Expr::BinOp {
                    op: BinOp::ElemPow,
                    lhs: Box::new(base),
                    rhs: Box::new(exp),
                })
            }
            _ => Ok(base),
        }
    }

    // postfix = primary ("'" | ".'" | "." IDENT ["(" args ")"] | "(" args ")")*
    fn parse_postfix(&mut self) -> Result<Expr, ScriptError> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek_token() {
                Token::Apostrophe => {
                    self.advance();
                    expr = Expr::Transpose(Box::new(expr));
                }
                Token::DotApostrophe => {
                    self.advance();
                    expr = Expr::NonConjTranspose(Box::new(expr));
                }
                // Chained indexing: expr(args) — e.g. f(a,b)(i)
                Token::LParen if !matches!(expr, Expr::Var(_)) => {
                    self.advance(); // consume '('
                    self.skip_newlines();
                    let args = if self.peek_token() == &Token::RParen {
                        vec![]
                    } else {
                        self.parse_arglist()?
                    };
                    self.skip_newlines();
                    self.expect(&Token::RParen)?;
                    expr = Expr::Index {
                        expr: Box::new(expr),
                        args,
                    };
                }
                Token::Dot => {
                    self.advance(); // consume '.'
                    let field = match self.peek_token().clone() {
                        Token::Ident(name) => {
                            self.advance();
                            name
                        }
                        other => {
                            return Err(ScriptError::Parse {
                                line: self.current_line(),
                                msg: format!("expected field name after '.', got {:?}", other),
                            })
                        }
                    };
                    if self.peek_token() == &Token::LParen {
                        // Method-call sugar: obj.method(args) → method(obj, args)
                        self.advance(); // consume '('
                        self.skip_newlines();
                        let mut args = vec![expr];
                        if self.peek_token() != &Token::RParen {
                            args.extend(self.parse_arglist()?);
                        }
                        self.skip_newlines();
                        self.expect(&Token::RParen)?;
                        expr = Expr::Call { name: field, args };
                    } else {
                        expr = Expr::Field {
                            object: Box::new(expr),
                            field,
                        };
                    }
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    // primary = NUMBER | STRING | IDENT | IDENT '(' arglist? ')' | '[' ... ']' | '(' expr ')' | '-' primary
    fn parse_primary(&mut self) -> Result<Expr, ScriptError> {
        match self.peek_token().clone() {
            Token::Number(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Token::Ident(name) => {
                self.advance();
                // Check if this is a function call
                if self.peek_token() == &Token::LParen {
                    self.advance(); // consume '('
                    self.skip_newlines();
                    let args = if self.peek_token() == &Token::RParen {
                        vec![]
                    } else {
                        self.parse_arglist()?
                    };
                    self.skip_newlines();
                    self.expect(&Token::RParen)?;
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Token::LBracket => {
                self.advance(); // consume '['
                self.skip_newlines();
                // Parse rows separated by semicolons
                let mut rows: Vec<Vec<Expr>> = Vec::new();
                if self.peek_token() != &Token::RBracket {
                    let first_row = self.parse_row()?;
                    rows.push(first_row);
                    loop {
                        self.skip_newlines();
                        if self.peek_token() == &Token::Semicolon {
                            self.advance();
                            self.skip_newlines();
                            if self.peek_token() == &Token::RBracket {
                                break;
                            }
                            let row = self.parse_row()?;
                            rows.push(row);
                        } else {
                            break;
                        }
                    }
                }
                self.skip_newlines();
                self.expect(&Token::RBracket)?;
                Ok(Expr::Matrix(rows))
            }
            Token::LBrace => {
                self.advance(); // consume '{'
                self.skip_newlines();
                let mut elems: Vec<Expr> = Vec::new();
                if self.peek_token() != &Token::RBrace {
                    elems.push(self.parse_range_expr()?);
                    loop {
                        self.skip_newlines();
                        if self.peek_token() == &Token::Comma {
                            self.advance();
                            self.skip_newlines();
                            if self.peek_token() == &Token::RBrace {
                                break;
                            }
                            elems.push(self.parse_range_expr()?);
                        } else {
                            break;
                        }
                    }
                }
                self.skip_newlines();
                self.expect(&Token::RBrace)?;
                Ok(Expr::CellArray(elems))
            }
            Token::LParen => {
                self.advance(); // consume '('
                self.skip_newlines();
                let expr = self.parse_range_expr()?;
                self.skip_newlines();
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::At => {
                self.advance(); // consume '@'
                if self.peek_token() == &Token::LParen {
                    // @(params) body_expr
                    self.advance(); // consume '('
                    let params = if self.peek_token() == &Token::RParen {
                        vec![]
                    } else {
                        self.parse_param_list()?
                    };
                    self.expect(&Token::RParen)?;
                    let body = self.parse_range_expr()?;
                    Ok(Expr::Lambda {
                        params,
                        body: Box::new(body),
                    })
                } else {
                    // @name
                    match self.peek_token().clone() {
                        Token::Ident(name) => {
                            self.advance();
                            Ok(Expr::FuncHandle(name))
                        }
                        other => Err(ScriptError::Parse {
                            line: self.current_line(),
                            msg: format!(
                                "expected function name or '(' after '@', got {:?}",
                                other
                            ),
                        }),
                    }
                }
            }
            // `end` used as an index variable inside subscripts (e.g. v(end), v(2:end))
            Token::End => {
                self.advance();
                Ok(Expr::Var("end".to_string()))
            }
            other => Err(ScriptError::Parse {
                line: self.current_line(),
                msg: format!("unexpected token in expression: {:?}", other),
            }),
        }
    }

    fn parse_arglist(&mut self) -> Result<Vec<Expr>, ScriptError> {
        let mut args = Vec::new();
        self.skip_newlines();
        args.push(self.parse_index_arg()?);
        loop {
            self.skip_newlines();
            if self.peek_token() == &Token::Comma {
                self.advance();
                self.skip_newlines();
                args.push(self.parse_index_arg()?);
            } else {
                break;
            }
        }
        Ok(args)
    }

    /// Parse one argument, treating a bare `:` as `Expr::All` (the "all elements" index).
    fn parse_index_arg(&mut self) -> Result<Expr, ScriptError> {
        if self.peek_token() == &Token::Colon {
            let next = self.tokens.get(self.pos + 1).map(|s| &s.token);
            if matches!(
                next,
                Some(Token::Comma)
                    | Some(Token::RParen)
                    | Some(Token::Newline)
                    | Some(Token::Eof)
                    | None
            ) {
                self.advance(); // consume ':'
                return Ok(Expr::All);
            }
        }
        self.parse_range_expr()
    }

    fn parse_row(&mut self) -> Result<Vec<Expr>, ScriptError> {
        let mut elems = Vec::new();
        elems.push(self.parse_range_expr()?);
        loop {
            self.skip_newlines();
            if self.peek_token() == &Token::Comma {
                self.advance();
                self.skip_newlines();
                elems.push(self.parse_range_expr()?);
            } else {
                break;
            }
        }
        Ok(elems)
    }
}
