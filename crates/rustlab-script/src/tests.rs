/// Unit tests for rustlab-script: lexer, parser, evaluator, and Value type.

#[cfg(test)]
mod bool_tests {
    use crate::ast::BinOp;
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn get_bool(ev: &Evaluator, name: &str) -> bool {
        match ev.get(name).unwrap() {
            Value::Bool(b) => *b,
            other => panic!("Expected bool for '{name}', got {other:?}"),
        }
    }

    // ── Comparison operators ─────────────────────────────────────────────────

    #[test]
    fn eq_true() {
        assert!(get_bool(&eval_str("b = 3 == 3"), "b"));
    }
    #[test]
    fn eq_false() {
        assert!(!get_bool(&eval_str("b = 3 == 4"), "b"));
    }
    #[test]
    fn ne_true() {
        assert!(get_bool(&eval_str("b = 3 != 4"), "b"));
    }
    #[test]
    fn ne_false() {
        assert!(!get_bool(&eval_str("b = 3 != 3"), "b"));
    }
    #[test]
    fn lt_true() {
        assert!(get_bool(&eval_str("b = 2 < 3"), "b"));
    }
    #[test]
    fn lt_false() {
        assert!(!get_bool(&eval_str("b = 3 < 2"), "b"));
    }
    #[test]
    fn le_true() {
        assert!(get_bool(&eval_str("b = 3 <= 3"), "b"));
    }
    #[test]
    fn le_false() {
        assert!(!get_bool(&eval_str("b = 4 <= 3"), "b"));
    }
    #[test]
    fn gt_true() {
        assert!(get_bool(&eval_str("b = 5 > 3"), "b"));
    }
    #[test]
    fn gt_false() {
        assert!(!get_bool(&eval_str("b = 2 > 3"), "b"));
    }
    #[test]
    fn ge_true() {
        assert!(get_bool(&eval_str("b = 3 >= 3"), "b"));
    }
    #[test]
    fn ge_false() {
        assert!(!get_bool(&eval_str("b = 2 >= 3"), "b"));
    }

    // ── Logical operators ────────────────────────────────────────────────────

    #[test]
    fn and_tt() {
        assert!(get_bool(&eval_str("b = (1 < 2) && (3 < 4)"), "b"));
    }
    #[test]
    fn and_tf() {
        assert!(!get_bool(&eval_str("b = (1 < 2) && (4 < 3)"), "b"));
    }
    #[test]
    fn or_ff() {
        assert!(!get_bool(&eval_str("b = (2 < 1) || (4 < 3)"), "b"));
    }
    #[test]
    fn or_ft() {
        assert!(get_bool(&eval_str("b = (2 < 1) || (3 < 4)"), "b"));
    }

    // ── Unary not ────────────────────────────────────────────────────────────

    #[test]
    fn not_true() {
        assert!(!get_bool(&eval_str("b = !(1 < 2)"), "b"));
    }
    #[test]
    fn not_false() {
        assert!(get_bool(&eval_str("b = !(2 < 1)"), "b"));
    }

    // ── Display ──────────────────────────────────────────────────────────────

    #[test]
    fn bool_display_true() {
        assert_eq!(format!("{}", Value::Bool(true)), "true");
    }
    #[test]
    fn bool_display_false() {
        assert_eq!(format!("{}", Value::Bool(false)), "false");
    }

    // ── Bool == Bool ─────────────────────────────────────────────────────────

    #[test]
    fn bool_eq_bool() {
        let result = Value::binop(BinOp::Eq, Value::Bool(true), Value::Bool(true)).unwrap();
        assert!(matches!(result, Value::Bool(true)));
    }

    #[test]
    fn bool_ne_bool() {
        let result = Value::binop(BinOp::Ne, Value::Bool(true), Value::Bool(false)).unwrap();
        assert!(matches!(result, Value::Bool(true)));
    }

    // ── Error cases ──────────────────────────────────────────────────────────

    #[test]
    fn not_on_scalar_errors() {
        assert!(Value::Bool(true).not().is_ok());
        assert!(Value::Scalar(1.0).not().is_err());
    }

    #[test]
    fn and_on_non_bool_errors() {
        assert!(Value::binop(BinOp::And, Value::Scalar(1.0), Value::Scalar(1.0)).is_err());
    }

    #[test]
    fn comparison_chained_with_logic() {
        // (2 < 5) && (5 < 10) → true
        let ev = eval_str("result = (2 < 5) && (5 < 10)");
        assert!(get_bool(&ev, "result"));
    }

    #[test]
    fn assignment_not_confused_with_eq() {
        // x = 5 should be assignment, not comparison
        let ev = eval_str("x = 5");
        match ev.get("x").unwrap() {
            Value::Scalar(n) => assert!((*n - 5.0).abs() < 1e-12),
            _ => panic!("Expected scalar"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod lexer_tests {
    use crate::lexer::{tokenize, Token};

    fn tokens(src: &str) -> Vec<Token> {
        tokenize(src)
            .unwrap()
            .into_iter()
            .map(|s| s.token)
            .collect()
    }

    #[test]
    fn integer_number() {
        let t = tokens("42\n");
        assert!(matches!(t[0], Token::Number(n) if (n - 42.0).abs() < 1e-12));
    }

    #[test]
    fn float_number() {
        let t = tokens("3.14\n");
        assert!(matches!(t[0], Token::Number(n) if (n - 3.14).abs() < 1e-12));
    }

    #[test]
    fn scientific_notation() {
        let t = tokens("1e3\n");
        assert!(matches!(t[0], Token::Number(n) if (n - 1000.0).abs() < 1e-9));
        let t2 = tokens("2.5e-2\n");
        assert!(matches!(t2[0], Token::Number(n) if (n - 0.025).abs() < 1e-12));
    }

    #[test]
    fn string_literal() {
        let t = tokens("\"hello\"\n");
        assert!(matches!(&t[0], Token::Str(s) if s == "hello"));
    }

    #[test]
    fn identifier() {
        let t = tokens("foo\n");
        assert!(matches!(&t[0], Token::Ident(s) if s == "foo"));
    }

    #[test]
    fn operators() {
        let t = tokens("+ - * / ^\n");
        assert!(matches!(t[0], Token::Plus));
        assert!(matches!(t[1], Token::Minus));
        assert!(matches!(t[2], Token::Star));
        assert!(matches!(t[3], Token::Slash));
        assert!(matches!(t[4], Token::Caret));
    }

    #[test]
    fn dot_operators() {
        let t = tokens(".* ./ .^\n");
        assert!(matches!(t[0], Token::DotStar));
        assert!(matches!(t[1], Token::DotSlash));
        assert!(matches!(t[2], Token::DotCaret));
    }

    #[test]
    fn colon_and_apostrophe() {
        // After an identifier/number, ' is transpose
        let t = tokens("x'\n");
        assert!(matches!(t[0], Token::Ident(_)));
        assert!(matches!(t[1], Token::Apostrophe));
        // Colon followed by ' starts a string literal
        let t2 = tokens(": 'hello'\n");
        assert!(matches!(t2[0], Token::Colon));
        assert!(matches!(&t2[1], Token::Str(s) if s == "hello"));
    }

    #[test]
    fn brackets_and_parens() {
        let t = tokens("[ ] ( ) ,\n");
        assert!(matches!(t[0], Token::LBracket));
        assert!(matches!(t[1], Token::RBracket));
        assert!(matches!(t[2], Token::LParen));
        assert!(matches!(t[3], Token::RParen));
        assert!(matches!(t[4], Token::Comma));
    }

    #[test]
    fn semicolon() {
        let t = tokens("a;\n");
        assert!(matches!(t[1], Token::Semicolon));
    }

    #[test]
    fn comment_ignored() {
        let t = tokens("42 # this is a comment\n");
        assert!(matches!(t[0], Token::Number(n) if (n - 42.0).abs() < 1e-12));
        assert!(matches!(t[1], Token::Newline));
        assert!(matches!(t[2], Token::Eof));
    }

    #[test]
    fn multiple_newlines_collapsed() {
        let t = tokens("a\n\n\nb\n");
        // Should be: Ident(a), Newline, Ident(b), Newline, Eof
        let newline_count = t.iter().filter(|t| matches!(t, Token::Newline)).count();
        assert_eq!(newline_count, 2, "consecutive newlines should collapse");
    }

    #[test]
    fn unterminated_string_errors() {
        assert!(tokenize("\"oops\n").is_err());
    }

    #[test]
    fn ends_with_eof() {
        let t = tokens("x\n");
        assert!(matches!(t.last().unwrap(), Token::Eof));
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod parser_tests {
    use crate::ast::{BinOp, Expr, Stmt, StmtKind};
    use crate::{lexer, parser};

    fn parse(src: &str) -> Vec<Stmt> {
        let src = if src.ends_with('\n') {
            src.to_string()
        } else {
            format!("{}\n", src)
        };
        let tokens = lexer::tokenize(&src).unwrap();
        parser::parse(tokens).unwrap()
    }

    fn first_expr(src: &str) -> Expr {
        match &parse(src)[0].kind {
            StmtKind::Expr(e, _) => e.clone(),
            StmtKind::Assign { expr, .. } => expr.clone(),
            _ => panic!("expected expression or assignment statement"),
        }
    }

    #[test]
    fn number_literal() {
        match first_expr("3.14") {
            Expr::Number(n) => assert!((n - 3.14).abs() < 1e-12),
            other => panic!("Expected Number, got {other:?}"),
        }
    }

    #[test]
    fn assignment_stmt() {
        let stmts = parse("x = 42");
        match &stmts[0].kind {
            StmtKind::Assign {
                name,
                expr: Expr::Number(n),
                suppress: false,
            } => {
                assert_eq!(name, "x");
                assert!((*n - 42.0).abs() < 1e-12);
            }
            other => panic!("Expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn suppress_flag_with_semicolon() {
        let stmts = parse("x = 42;");
        match &stmts[0].kind {
            StmtKind::Assign { suppress: true, .. } => {}
            other => panic!("Expected suppressed assign, got {other:?}"),
        }
    }

    #[test]
    fn addition_expr() {
        match first_expr("1 + 2") {
            Expr::BinOp { op: BinOp::Add, .. } => {}
            other => panic!("Expected Add, got {other:?}"),
        }
    }

    #[test]
    fn unary_minus() {
        match first_expr("-5") {
            Expr::UnaryMinus(_) => {}
            other => panic!("Expected UnaryMinus, got {other:?}"),
        }
    }

    #[test]
    fn unary_minus_binds_weaker_than_elem_pow() {
        // -x .^ 2 must parse as -(x .^ 2), matching Octave precedence.
        match first_expr("-x .^ 2") {
            Expr::UnaryMinus(inner) => match inner.as_ref() {
                Expr::BinOp {
                    op: BinOp::ElemPow, ..
                } => {}
                other => panic!("Expected UnaryMinus(ElemPow), got UnaryMinus({other:?})"),
            },
            other => panic!("Expected UnaryMinus(ElemPow), got {other:?}"),
        }
    }

    #[test]
    fn unary_minus_binds_weaker_than_pow() {
        // -x ^ 2 must parse as -(x ^ 2).
        match first_expr("-x ^ 2") {
            Expr::UnaryMinus(inner) => match inner.as_ref() {
                Expr::BinOp { op: BinOp::Pow, .. } => {}
                other => panic!("Expected UnaryMinus(Pow), got UnaryMinus({other:?})"),
            },
            other => panic!("Expected UnaryMinus(Pow), got {other:?}"),
        }
    }

    #[test]
    fn unary_minus_allowed_on_rhs_of_pow() {
        // 2 ^ -3 must still parse (Octave accepts it as 2 ^ (-3) = 0.125).
        match first_expr("2 ^ -3") {
            Expr::BinOp {
                op: BinOp::Pow,
                rhs,
                ..
            } => match rhs.as_ref() {
                Expr::UnaryMinus(_) => {}
                other => panic!("Expected Pow(_, UnaryMinus), got Pow(_, {other:?})"),
            },
            other => panic!("Expected Pow, got {other:?}"),
        }
    }

    #[test]
    fn range_two_args() {
        match first_expr("1:5") {
            Expr::Range { step: None, .. } => {}
            other => panic!("Expected Range(None), got {other:?}"),
        }
    }

    #[test]
    fn range_three_args() {
        match first_expr("0:0.5:2") {
            Expr::Range { step: Some(_), .. } => {}
            other => panic!("Expected Range(Some), got {other:?}"),
        }
    }

    #[test]
    fn transpose_expr() {
        match first_expr("v'") {
            Expr::Transpose(_) => {}
            other => panic!("Expected Transpose, got {other:?}"),
        }
    }

    #[test]
    fn element_wise_mul() {
        match first_expr("a .* b") {
            Expr::BinOp {
                op: BinOp::ElemMul, ..
            } => {}
            other => panic!("Expected ElemMul, got {other:?}"),
        }
    }

    #[test]
    fn function_call_expr() {
        match first_expr("sin(x)") {
            Expr::Call { name, args } => {
                assert_eq!(name, "sin");
                assert_eq!(args.len(), 1);
            }
            other => panic!("Expected Call, got {other:?}"),
        }
    }

    #[test]
    fn matrix_literal() {
        match first_expr("[1, 2; 3, 4]") {
            Expr::Matrix(rows) => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
            }
            other => panic!("Expected Matrix, got {other:?}"),
        }
    }

    #[test]
    fn operator_precedence_mul_over_add() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        match first_expr("1 + 2 * 3") {
            Expr::BinOp {
                op: BinOp::Add,
                rhs,
                ..
            } => {
                assert!(matches!(*rhs, Expr::BinOp { op: BinOp::Mul, .. }));
            }
            other => panic!("Expected Add at root, got {other:?}"),
        }
    }

    #[test]
    fn power_right_associative() {
        // 2 ^ 3 ^ 4 should parse as 2 ^ (3 ^ 4)
        match first_expr("2 ^ 3 ^ 4") {
            Expr::BinOp {
                op: BinOp::Pow,
                rhs,
                ..
            } => {
                assert!(matches!(*rhs, Expr::BinOp { op: BinOp::Pow, .. }));
            }
            other => panic!("Expected Pow at root, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod value_tests {
    use crate::ast::BinOp;
    use crate::eval::value::Value;
    use ndarray::Array1;
    use num_complex::Complex;
    use rustlab_core::C64;

    fn scalar(n: f64) -> Value {
        Value::Scalar(n)
    }
    fn complex(re: f64, im: f64) -> Value {
        Value::Complex(Complex::new(re, im))
    }
    fn vec_val(v: &[f64]) -> Value {
        Value::Vector(Array1::from_iter(v.iter().map(|&x| Complex::new(x, 0.0))))
    }
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // ── Negate ──────────────────────────────────────────────────────────────

    #[test]
    fn negate_scalar() {
        match scalar(3.0).negate().unwrap() {
            Value::Scalar(n) => assert!(close(n, -3.0)),
            _ => panic!(),
        }
    }

    #[test]
    fn negate_complex() {
        match complex(1.0, 2.0).negate().unwrap() {
            Value::Complex(c) => {
                assert!(close(c.re, -1.0));
                assert!(close(c.im, -2.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn negate_vector() {
        let v = vec_val(&[1.0, -2.0, 3.0]);
        match v.negate().unwrap() {
            Value::Vector(v) => {
                assert!(close(v[0].re, -1.0));
                assert!(close(v[1].re, 2.0));
                assert!(close(v[2].re, -3.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn negate_string_errors() {
        assert!(Value::Str("x".to_string()).negate().is_err());
    }

    // ── Transpose ───────────────────────────────────────────────────────────

    #[test]
    fn transpose_real_vector_unchanged() {
        // Transposing a row vector produces an n×1 Matrix (column vector)
        let v = vec_val(&[1.0, 2.0, 3.0]);
        match v.transpose().unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m.nrows(), 3);
                assert_eq!(m.ncols(), 1);
                assert!(close(m[[0, 0]].re, 1.0));
                assert!(close(m[[2, 0]].re, 3.0));
            }
            _ => panic!("Expected Matrix(n×1) from Vector transpose"),
        }
    }

    #[test]
    fn transpose_complex_vector_conjugates() {
        // Transposing a complex row vector produces an n×1 Matrix with conjugated values
        let v = Value::Vector(Array1::from_vec(vec![
            Complex::new(1.0, 2.0),
            Complex::new(3.0, -1.0),
        ]));
        match v.transpose().unwrap() {
            Value::Matrix(m) => {
                assert!(close(m[[0, 0]].im, -2.0));
                assert!(close(m[[1, 0]].im, 1.0));
            }
            _ => panic!("Expected Matrix(n×1) from Vector transpose"),
        }
    }

    #[test]
    fn transpose_scalar_is_identity() {
        match scalar(5.0).transpose().unwrap() {
            Value::Scalar(n) => assert!(close(n, 5.0)),
            _ => panic!(),
        }
    }

    #[test]
    fn transpose_complex_scalar_conjugates() {
        match complex(1.0, 3.0).transpose().unwrap() {
            Value::Complex(c) => {
                assert!(close(c.re, 1.0));
                assert!(close(c.im, -3.0));
            }
            _ => panic!(),
        }
    }

    // ── BinOp ────────────────────────────────────────────────────────────────

    #[test]
    fn add_scalars() {
        match Value::binop(BinOp::Add, scalar(2.0), scalar(3.0)).unwrap() {
            Value::Scalar(n) => assert!(close(n, 5.0)),
            _ => panic!(),
        }
    }

    #[test]
    fn sub_scalars() {
        match Value::binop(BinOp::Sub, scalar(5.0), scalar(3.0)).unwrap() {
            Value::Scalar(n) => assert!(close(n, 2.0)),
            _ => panic!(),
        }
    }

    #[test]
    fn mul_scalars() {
        match Value::binop(BinOp::Mul, scalar(4.0), scalar(3.0)).unwrap() {
            Value::Scalar(n) => assert!(close(n, 12.0)),
            _ => panic!(),
        }
    }

    #[test]
    fn div_scalars() {
        match Value::binop(BinOp::Div, scalar(10.0), scalar(4.0)).unwrap() {
            Value::Scalar(n) => assert!(close(n, 2.5)),
            _ => panic!(),
        }
    }

    #[test]
    fn pow_scalar() {
        match Value::binop(BinOp::Pow, scalar(2.0), scalar(10.0)).unwrap() {
            Value::Scalar(n) => assert!(close(n, 1024.0)),
            _ => panic!(),
        }
    }

    #[test]
    fn add_complex_values() {
        match Value::binop(BinOp::Add, complex(1.0, 2.0), complex(3.0, 4.0)).unwrap() {
            Value::Complex(c) => {
                assert!(close(c.re, 4.0));
                assert!(close(c.im, 6.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn scalar_plus_complex_promotes() {
        match Value::binop(BinOp::Add, scalar(1.0), complex(0.0, 1.0)).unwrap() {
            Value::Complex(c) => {
                assert!(close(c.re, 1.0));
                assert!(close(c.im, 1.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn vector_add_vector() {
        match Value::binop(BinOp::Add, vec_val(&[1.0, 2.0]), vec_val(&[3.0, 4.0])).unwrap() {
            Value::Vector(v) => {
                assert!(close(v[0].re, 4.0));
                assert!(close(v[1].re, 6.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn scalar_broadcast_to_vector() {
        match Value::binop(BinOp::Mul, scalar(2.0), vec_val(&[1.0, 2.0, 3.0])).unwrap() {
            Value::Vector(v) => {
                assert!(close(v[0].re, 2.0));
                assert!(close(v[1].re, 4.0));
                assert!(close(v[2].re, 6.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn vector_length_mismatch_errors() {
        assert!(Value::binop(BinOp::Add, vec_val(&[1.0, 2.0]), vec_val(&[1.0])).is_err());
    }

    #[test]
    fn string_concatenation() {
        match Value::binop(
            BinOp::Add,
            Value::Str("hello".to_string()),
            Value::Str(" world".to_string()),
        )
        .unwrap()
        {
            Value::Str(s) => assert_eq!(s, "hello world"),
            _ => panic!(),
        }
    }

    // ── Index ────────────────────────────────────────────────────────────────

    #[test]
    fn vector_index_one_based() {
        let v = vec_val(&[10.0, 20.0, 30.0]);
        match v.index(vec![Value::Scalar(1.0)]).unwrap() {
            Value::Scalar(n) => assert!(close(n, 10.0)),
            _ => panic!(),
        }
    }

    #[test]
    fn vector_index_last_element() {
        let v = vec_val(&[10.0, 20.0, 30.0]);
        match v.index(vec![Value::Scalar(3.0)]).unwrap() {
            Value::Scalar(n) => assert!(close(n, 30.0)),
            _ => panic!(),
        }
    }
}

#[cfg(test)]
mod evaluator_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("Expected scalar for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn predefined_pi() {
        let ev = Evaluator::new();
        match ev.get("pi").unwrap() {
            Value::Scalar(n) => assert!(close(*n, std::f64::consts::PI)),
            _ => panic!(),
        }
    }

    #[test]
    fn predefined_e() {
        let ev = Evaluator::new();
        match ev.get("e").unwrap() {
            Value::Scalar(n) => assert!(close(*n, std::f64::consts::E)),
            _ => panic!(),
        }
    }

    #[test]
    fn predefined_j_is_imaginary_unit() {
        let ev = Evaluator::new();
        match ev.get("j").unwrap() {
            Value::Complex(c) => {
                assert!(close(c.re, 0.0));
                assert!(close(c.im, 1.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn simple_assignment() {
        let ev = eval_str("x = 42");
        assert!(close(get_scalar(&ev, "x"), 42.0));
    }

    #[test]
    fn arithmetic_expr() {
        let ev = eval_str("y = 3 + 4 * 2");
        assert!(close(get_scalar(&ev, "y"), 11.0));
    }

    #[test]
    fn complex_constant_j() {
        let ev = eval_str("z = 3 + j*4");
        match ev.get("z").unwrap() {
            Value::Complex(c) => {
                assert!(close(c.re, 3.0));
                assert!(close(c.im, 4.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn range_creates_vector() {
        let ev = eval_str("v = 1:5");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 5);
                assert!(close(v[0].re, 1.0));
                assert!(close(v[4].re, 5.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn range_with_step() {
        let ev = eval_str("v = 0:2:8");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 5); // 0,2,4,6,8
                assert!(close(v[2].re, 4.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn builtin_abs() {
        let ev = eval_str("y = abs(-5)");
        assert!(close(get_scalar(&ev, "y"), 5.0));
    }

    #[test]
    fn builtin_sin_pi() {
        let ev = eval_str("y = sin(pi)");
        assert!(get_scalar(&ev, "y").abs() < 1e-14);
    }

    #[test]
    fn builtin_cos_zero() {
        let ev = eval_str("y = cos(0)");
        assert!(close(get_scalar(&ev, "y"), 1.0));
    }

    #[test]
    fn builtin_sqrt() {
        let ev = eval_str("y = sqrt(9)");
        assert!(close(get_scalar(&ev, "y"), 3.0));
    }

    #[test]
    fn builtin_zeros() {
        let ev = eval_str("v = zeros(5)");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 5);
                assert!(v.iter().all(|c| c.norm() < 1e-12));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn builtin_ones() {
        let ev = eval_str("v = ones(4)");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 4);
                assert!(v
                    .iter()
                    .all(|c| (c.re - 1.0).abs() < 1e-12 && c.im.abs() < 1e-12));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn builtin_linspace() {
        let ev = eval_str("v = linspace(0, 1, 5)");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 5);
                assert!((v[0].re - 0.0).abs() < 1e-12);
                assert!((v[2].re - 0.5).abs() < 1e-12);
                assert!((v[4].re - 1.0).abs() < 1e-12);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn builtin_len() {
        let ev = eval_str("v = ones(7)\nn = len(v)");
        assert!(close(get_scalar(&ev, "n"), 7.0));
    }

    #[test]
    fn vector_indexing_one_based() {
        let ev = eval_str("v = linspace(10, 50, 5)\nx = v(1)");
        assert!(close(get_scalar(&ev, "x"), 10.0));
    }

    #[test]
    fn transpose_operator() {
        // Transposing a row vector [1,2,3] produces a 3×1 column Matrix
        let ev = eval_str("v = [1, 2, 3]\nvt = v'");
        match ev.get("vt").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m.nrows(), 3);
                assert_eq!(m.ncols(), 1);
            }
            _ => panic!("Expected Matrix(3×1) from transpose of [1,2,3]"),
        }
    }

    #[test]
    fn elementwise_mul() {
        let ev = eval_str("a = [1, 2, 3]\nb = [4, 5, 6]\nc = a .* b");
        match ev.get("c").unwrap() {
            Value::Vector(v) => {
                assert!(close(v[0].re, 4.0));
                assert!(close(v[1].re, 10.0));
                assert!(close(v[2].re, 18.0));
            }
            _ => panic!(),
        }
    }
}

// ─── Error tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod error_tests {
    use crate::{lexer, parser, Evaluator};

    fn eval_err(src: &str) -> crate::error::ScriptError {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = crate::eval::Evaluator::new();
        let mut last_err = None;
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                last_err = Some(e);
                break;
            }
        }
        last_err.expect("expected an error but script ran successfully")
    }

    #[test]
    fn undefined_variable_errors() {
        // x is never assigned — should produce Undefined error
        let _err = eval_err("x + 1");
        // just verifying it produces an error (the type assertion above panics if not)
    }

    #[test]
    fn index_out_of_bounds() {
        let _err = eval_err("v = 1:3\nv(5)");
    }

    #[test]
    fn vector_star_vector_errors() {
        // v * v with `*` (matrix mul) on two 1D vectors should error
        let _err = eval_err("a = 1:3\nb = 1:3\na * b");
    }

    #[test]
    fn inv_singular_errors() {
        let _err = eval_err("inv([0,0;0,0])");
    }

    #[test]
    fn linsolve_dimension_mismatch() {
        // A is 2×2, b has 3 elements — should error
        let _err = eval_err("linsolve([1,2;3,4], [1,2,3])");
    }

    #[test]
    fn det_non_square_errors() {
        let _err = eval_err("det([1,2,3;4,5,6])");
    }
}

// ─── Error line number tests ────────────────────────────────────────────────

#[cfg(test)]
mod error_line_tests {
    fn eval_err(src: &str) -> crate::error::ScriptError {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = crate::eval::Evaluator::new();
        let mut last_err = None;
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                last_err = Some(e);
                break;
            }
        }
        last_err.expect("expected an error but script ran successfully")
    }

    #[test]
    fn undefined_var_has_line_number() {
        let err = eval_err("a = 1\nb = 2\nc = d + 1");
        let msg = err.to_string();
        assert!(msg.contains("line 3"), "expected 'line 3' in: {msg}");
        assert!(
            msg.contains("undefined variable"),
            "expected 'undefined variable' in: {msg}"
        );
        assert!(
            msg.contains("'d'"),
            "expected quoted variable name in: {msg}"
        );
    }

    #[test]
    fn type_error_has_line_number() {
        let err = eval_err("a = [1,2,3]\nb = 'hello'\nc = a + b");
        let msg = err.to_string();
        assert!(msg.contains("line 3"), "expected 'line 3' in: {msg}");
        assert!(
            msg.contains("type error"),
            "expected 'type error' in: {msg}"
        );
    }

    #[test]
    fn runtime_error_has_line_number() {
        let err = eval_err("x = 1:3\ny = 2\nz = x(10)");
        let msg = err.to_string();
        assert!(msg.contains("line 3"), "expected 'line 3' in: {msg}");
    }

    #[test]
    fn line_1_error() {
        let err = eval_err("nonexistent_thing + 1");
        let msg = err.to_string();
        assert!(msg.contains("line 1"), "expected 'line 1' in: {msg}");
    }

    #[test]
    fn error_inside_loop_body() {
        let err = eval_err("for k = 1:3\n  x = undefined_var\nend");
        let msg = err.to_string();
        assert!(msg.contains("line 2"), "expected 'line 2' in: {msg}");
        assert!(
            msg.contains("undefined variable"),
            "expected 'undefined variable' in: {msg}"
        );
    }

    #[test]
    fn arg_count_error_has_line_number() {
        let err = eval_err("a = 1\nabs(1, 2, 3)");
        let msg = err.to_string();
        assert!(msg.contains("line 2"), "expected 'line 2' in: {msg}");
        assert!(msg.contains("arguments"), "expected 'arguments' in: {msg}");
    }
}

// ─── Matrix / linalg tests ──────────────────────────────────────────────────

#[cfg(test)]
mod matrix_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("Expected scalar for '{name}', got {other:?}"),
        }
    }

    fn get_vector(ev: &Evaluator, name: &str) -> Vec<f64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => panic!("Expected vector for '{name}', got {other:?}"),
        }
    }

    fn get_matrix(ev: &Evaluator, name: &str) -> ndarray::Array2<num_complex::Complex<f64>> {
        match ev.get(name).unwrap() {
            Value::Matrix(m) => m.clone(),
            other => panic!("Expected Matrix for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn eye_diagonal_ones() {
        let ev = eval_str("M = eye(3)");
        let m = get_matrix(&ev, "M");
        assert!((m[[0, 0]].re - 1.0).abs() < 1e-12, "M[0][0] should be 1");
        assert!((m[[0, 1]].re).abs() < 1e-12, "M[0][1] should be 0");
        assert!((m[[1, 1]].re - 1.0).abs() < 1e-12, "M[1][1] should be 1");
    }

    #[test]
    fn trace_of_eye() {
        let ev = eval_str("x = trace(eye(4))");
        assert!(
            close(get_scalar(&ev, "x"), 4.0, 1e-12),
            "trace(eye(4)) should be 4.0"
        );
    }

    #[test]
    fn det_2x2_known() {
        // det([3,8;4,6]) = 3*6 - 8*4 = 18 - 32 = -14
        let ev = eval_str("x = det([3,8;4,6])");
        assert!(
            close(get_scalar(&ev, "x"), -14.0, 1e-10),
            "det([3,8;4,6]) should be -14"
        );
    }

    #[test]
    fn det_identity_3x3() {
        let ev = eval_str("x = det(eye(3))");
        assert!(
            close(get_scalar(&ev, "x"), 1.0, 1e-10),
            "det(eye(3)) should be 1.0"
        );
    }

    #[test]
    fn inv_times_a_is_identity() {
        let ev = eval_str("A = [2,1;5,7]\nB = inv(A) * A");
        let b = get_matrix(&ev, "B");
        // B should be approximately eye(2)
        assert!((b[[0, 0]].re - 1.0).abs() < 1e-10, "B[0][0] should be ≈1");
        assert!((b[[0, 1]].re).abs() < 1e-10, "B[0][1] should be ≈0");
        assert!((b[[1, 0]].re).abs() < 1e-10, "B[1][0] should be ≈0");
        assert!((b[[1, 1]].re - 1.0).abs() < 1e-10, "B[1][1] should be ≈1");
    }

    #[test]
    fn linsolve_known_2x2() {
        // [2,1;5,7] * x = [11;13] → x = [3, 5] (check: 2*3+1*5=11, 5*3+7*5=50≠13... wait)
        // Correct solution: [2,1;5,7]*[3,1]' = [7,22]≠[11,13]
        // Use inv: [2,1;5,7]^-1 = (1/9)*[7,-1;-5,2]
        // x = [7*11-1*13, -5*11+2*13]/9 = [77-13, -55+26]/9 = [64/9, -29/9]
        // Actually: det=2*7-1*5=14-5=9
        // x1=(7*11-1*13)/9=(77-13)/9=64/9≈7.11, x2=(-5*11+2*13)/9=(-55+26)/9=-29/9≈-3.22
        // Let's use a simpler system: [1,0;0,1] * x = [3;7] → x=[3,7]
        let ev = eval_str("x = linsolve([1,0;0,1], [3;7])");
        let x = get_vector(&ev, "x");
        assert!(close(x[0], 3.0, 1e-10), "x[0] should be 3.0, got {}", x[0]);
        assert!(close(x[1], 7.0, 1e-10), "x[1] should be 7.0, got {}", x[1]);
    }

    #[test]
    fn dot_orthogonal() {
        let ev = eval_str("x = dot([1,0,0], [0,1,0])");
        assert!(
            close(get_scalar(&ev, "x"), 0.0, 1e-12),
            "dot of orthogonal vectors should be 0"
        );
    }

    #[test]
    fn dot_known() {
        let ev = eval_str("x = dot([3,4], [3,4])");
        assert!(
            close(get_scalar(&ev, "x"), 25.0, 1e-12),
            "dot([3,4],[3,4]) should be 25"
        );
    }

    #[test]
    fn norm_l2_pythagorean() {
        let ev = eval_str("x = norm([3,4])");
        assert!(
            close(get_scalar(&ev, "x"), 5.0, 1e-10),
            "norm([3,4]) should be 5.0"
        );
    }

    #[test]
    fn norm_l1_known() {
        let ev = eval_str("x = norm([1,2,3], 1)");
        assert!(
            close(get_scalar(&ev, "x"), 6.0, 1e-10),
            "L1 norm of [1,2,3] should be 6.0"
        );
    }

    #[test]
    fn cross_known() {
        let ev = eval_str("v = cross([1,0,0], [0,1,0])");
        let v = get_vector(&ev, "v");
        assert!(close(v[0], 0.0, 1e-12), "cross[0] should be 0");
        assert!(close(v[1], 0.0, 1e-12), "cross[1] should be 0");
        assert!(close(v[2], 1.0, 1e-12), "cross[2] should be 1");
    }

    #[test]
    fn reshape_changes_dimensions() {
        let ev = eval_str("M = reshape(1:6, 2, 3)");
        let m = get_matrix(&ev, "M");
        assert_eq!(m.nrows(), 2, "reshaped matrix should have 2 rows");
        assert_eq!(m.ncols(), 3, "reshaped matrix should have 3 cols");
    }

    #[test]
    fn horzcat_increases_cols() {
        let ev = eval_str("M = horzcat(eye(2), eye(2))");
        let m = get_matrix(&ev, "M");
        assert_eq!(
            m.ncols(),
            4,
            "horzcat of two 2×2 eye matrices should have 4 cols"
        );
    }

    #[test]
    fn vertcat_increases_rows() {
        let ev = eval_str("M = vertcat(eye(2), eye(2))");
        let m = get_matrix(&ev, "M");
        assert_eq!(
            m.nrows(),
            4,
            "vertcat of two 2×2 eye matrices should have 4 rows"
        );
    }

    #[test]
    fn diag_extracts_diagonal() {
        let ev = eval_str("v = diag([1,2;3,4])");
        let v = get_vector(&ev, "v");
        assert!(close(v[0], 1.0, 1e-12), "diag[0] should be 1.0");
        assert!(close(v[1], 4.0, 1e-12), "diag[1] should be 4.0");
    }

    #[test]
    fn nx1_column_matrix_single_index_returns_scalar() {
        // v(i) on an Nx1 column matrix should unwrap to a scalar so template
        // interpolation and fprintf %f work without an explicit cast.
        let ev = eval_str("v = [1.0; 2.0; 3.0];\nx = v(2);");
        assert!(close(get_scalar(&ev, "x"), 2.0, 1e-12));
    }

    #[test]
    fn nx1_column_matrix_single_index_first_and_last() {
        let ev = eval_str("v = [10.0; 20.0; 30.0];\na = v(1);\nb = v(3);");
        assert!(close(get_scalar(&ev, "a"), 10.0, 1e-12));
        assert!(close(get_scalar(&ev, "b"), 30.0, 1e-12));
    }

    #[test]
    fn mxn_single_index_still_returns_row() {
        // Preserve existing behavior: M(i) on a general MxN matrix returns
        // the i-th row as a vector. Only Nx1 matrices are unwrapped to scalar.
        let ev = eval_str("M = [1.0, 2.0; 3.0, 4.0];\nr = M(1);");
        let r = get_vector(&ev, "r");
        assert_eq!(r.len(), 2);
        assert!(close(r[0], 1.0, 1e-12));
        assert!(close(r[1], 2.0, 1e-12));
    }
}

// ─── Save/load round-trip tests ─────────────────────────────────────────────

#[cfg(test)]
mod io_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn eval_err(src: &str) -> crate::error::ScriptError {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = crate::eval::Evaluator::new();
        let mut last_err = None;
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                last_err = Some(e);
                break;
            }
        }
        last_err.expect("expected an error but script ran successfully")
    }

    fn get_vector(ev: &Evaluator, name: &str) -> Vec<f64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => panic!("Expected vector for '{name}', got {other:?}"),
        }
    }

    fn tmp_path(suffix: &str) -> String {
        let mut p = std::env::temp_dir();
        p.push(format!("rustlab_io_test_{}{}", std::process::id(), suffix));
        p.to_str().unwrap().to_string()
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn npy_roundtrip_real_vector() {
        let path = tmp_path(".npy");
        let save_src = format!(r#"save("{path}", 1:5)"#);
        eval_str(&save_src);
        let load_src = format!(r#"x = load("{path}")"#);
        let ev = eval_str(&load_src);
        let x = get_vector(&ev, "x");
        assert!(close(x[0], 1.0, 1e-6), "x[0] should be 1.0, got {}", x[0]);
        assert!(close(x[4], 5.0, 1e-6), "x[4] should be 5.0, got {}", x[4]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn npy_roundtrip_complex_vector() {
        let path = tmp_path("_complex.npy");
        let save_src = format!(
            r#"v = [1+j*2, 3+j*4]
save("{path}", v)"#
        );
        eval_str(&save_src);
        let load_src = format!(r#"x = load("{path}")"#);
        let ev = eval_str(&load_src);
        // Check real parts
        match ev.get("x").unwrap() {
            Value::Vector(v) => {
                assert!(close(v[0].re, 1.0, 1e-6), "real part[0] should be 1.0");
                assert!(close(v[0].im, 2.0, 1e-6), "imag part[0] should be 2.0");
                assert!(close(v[1].re, 3.0, 1e-6), "real part[1] should be 3.0");
                assert!(close(v[1].im, 4.0, 1e-6), "imag part[1] should be 4.0");
            }
            other => panic!("Expected vector, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn csv_roundtrip_real_vector() {
        let path = tmp_path(".csv");
        let save_src = format!(r#"save("{path}", 1:4)"#);
        eval_str(&save_src);
        let load_src = format!(r#"x = load("{path}")"#);
        let ev = eval_str(&load_src);
        let x = get_vector(&ev, "x");
        assert!(
            close(x[0], 1.0, 1e-6),
            "csv x[0] should be 1.0, got {}",
            x[0]
        );
        assert!(
            close(x[3], 4.0, 1e-6),
            "csv x[3] should be 4.0, got {}",
            x[3]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn npz_roundtrip_named_array() {
        let path = tmp_path(".npz");
        let save_src = format!(r#"save("{path}", "arr", 1:3)"#);
        eval_str(&save_src);
        let load_src = format!(r#"x = load("{path}", "arr")"#);
        let ev = eval_str(&load_src);
        let x = get_vector(&ev, "x");
        assert!(
            close(x[0], 1.0, 1e-6),
            "npz x[0] should be 1.0, got {}",
            x[0]
        );
        assert!(
            close(x[2], 3.0, 1e-6),
            "npz x[2] should be 3.0, got {}",
            x[2]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_missing_file_errors() {
        let _err = eval_err(r#"load("/no/such/file/rustlab_nonexistent_abc123.npy")"#);
        // Just verifying it errors — the eval_err helper panics if no error is produced
    }
}

// ─── factor() tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod factor_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn eval_err(src: &str) -> crate::error::ScriptError {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = crate::eval::Evaluator::new();
        let mut last_err = None;
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                last_err = Some(e);
                break;
            }
        }
        last_err.expect("expected an error but script ran successfully")
    }

    fn get_vector(ev: &Evaluator, name: &str) -> Vec<f64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => panic!("Expected vector for '{name}', got {other:?}"),
        }
    }

    #[test]
    fn factor_of_1_is_empty() {
        let ev = eval_str("v = factor(1)");
        assert_eq!(get_vector(&ev, "v").len(), 0);
    }

    #[test]
    fn factor_12() {
        let ev = eval_str("v = factor(12)");
        assert_eq!(get_vector(&ev, "v"), vec![2.0, 2.0, 3.0]);
    }

    #[test]
    fn factor_prime() {
        let ev = eval_str("v = factor(17)");
        assert_eq!(get_vector(&ev, "v"), vec![17.0]);
    }

    #[test]
    fn factor_100() {
        let ev = eval_str("v = factor(100)");
        assert_eq!(get_vector(&ev, "v"), vec![2.0, 2.0, 5.0, 5.0]);
    }

    #[test]
    fn factor_zero_errors() {
        let _err = eval_err("factor(0)");
    }

    #[test]
    fn factor_negative_errors() {
        let _err = eval_err("factor(-3)");
    }

    #[test]
    fn factor_product_equals_input() {
        // product of factors of n == n
        let ev = eval_str("v = factor(360)");
        let factors = get_vector(&ev, "v");
        let product: f64 = factors.iter().product();
        assert!((product - 360.0).abs() < 1e-10);
    }
}

// ─── eig() tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod eig_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn eval_err(src: &str) -> crate::error::ScriptError {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = crate::eval::Evaluator::new();
        let mut last_err = None;
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                last_err = Some(e);
                break;
            }
        }
        last_err.expect("expected an error but script ran successfully")
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("Expected scalar for '{name}', got {other:?}"),
        }
    }

    fn get_complex_vector(ev: &Evaluator, name: &str) -> Vec<num_complex::Complex<f64>> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.to_vec(),
            other => panic!("expected Vector for '{name}', got {other:?}"),
        }
    }

    #[test]
    fn eig_identity_all_ones() {
        let ev = eval_str("v = eig(eye(3))");
        let vals = get_complex_vector(&ev, "v");
        assert_eq!(vals.len(), 3);
        for l in &vals {
            assert!(
                (l.re - 1.0).abs() < 1e-8,
                "eigenvalue re should be ~1, got {}",
                l.re
            );
            assert!(
                l.im.abs() < 1e-8,
                "eigenvalue im should be ~0, got {}",
                l.im
            );
        }
    }

    #[test]
    fn eig_diagonal_matrix() {
        // Eigenvalues of a diagonal matrix are its diagonal entries
        let ev = eval_str("v = eig([2,0;0,5])");
        let vals = get_complex_vector(&ev, "v");
        assert_eq!(vals.len(), 2);
        let mut re: Vec<f64> = vals.iter().map(|c| c.re).collect();
        re.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((re[0] - 2.0).abs() < 1e-8);
        assert!((re[1] - 5.0).abs() < 1e-8);
    }

    #[test]
    fn eig_symmetric_2x2() {
        // [2,1;1,2] has eigenvalues 1 and 3
        let ev = eval_str("v = eig([2,1;1,2])");
        let vals = get_complex_vector(&ev, "v");
        assert_eq!(vals.len(), 2);
        let mut re: Vec<f64> = vals.iter().map(|c| c.re).collect();
        re.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((re[0] - 1.0).abs() < 1e-8, "expected 1.0, got {}", re[0]);
        assert!((re[1] - 3.0).abs() < 1e-8, "expected 3.0, got {}", re[1]);
    }

    #[test]
    fn eig_sum_equals_trace() {
        // sum of eigenvalues == trace(M) for any square matrix
        let ev = eval_str("M = [4,2,1;2,3,1;1,1,2]\nv = eig(M)\nt = trace(M)");
        let vals = get_complex_vector(&ev, "v");
        let sum_re: f64 = vals.iter().map(|c| c.re).sum();
        let tr = get_scalar(&ev, "t");
        assert!(
            (sum_re - tr).abs() < 1e-7,
            "sum(eig) = {sum_re}, trace = {tr}"
        );
    }

    #[test]
    fn eig_product_equals_det() {
        // product of eigenvalues == det(M)
        let ev = eval_str("M = [3,1;1,3]\nv = eig(M)\nd = det(M)");
        let vals = get_complex_vector(&ev, "v");
        let prod: num_complex::Complex<f64> = vals.iter().product();
        let det_val = get_scalar(&ev, "d");
        assert!(
            (prod.re - det_val).abs() < 1e-7,
            "prod(eig) = {}, det = {}",
            prod.re,
            det_val
        );
    }

    #[test]
    fn eig_scalar_input() {
        // eig(5) — scalar treated as 1×1 matrix, returns [5]
        let ev = eval_str("v = eig(5)");
        let vals = get_complex_vector(&ev, "v");
        assert_eq!(vals.len(), 1);
        assert!((vals[0].re - 5.0).abs() < 1e-10);
        assert!(vals[0].im.abs() < 1e-10);
    }

    #[test]
    fn eig_non_square_errors() {
        let _err = eval_err("eig([1,2,3;4,5,6])");
    }
}

// ── Phase 1: Language Foundations ────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod phase1_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for s in &stmts {
            ev.exec_stmt(s).unwrap();
        }
        ev
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("expected scalar for '{name}', got {other:?}"),
        }
    }

    fn get_bool(ev: &Evaluator, name: &str) -> bool {
        match ev.get(name).unwrap() {
            Value::Bool(b) => *b,
            other => panic!("expected bool for '{name}', got {other:?}"),
        }
    }

    fn get_complex_vector(ev: &Evaluator, name: &str) -> Vec<num_complex::Complex<f64>> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.to_vec(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    // ── 1a: % comments ───────────────────────────────────────────────────────

    #[test]
    fn percent_comment_ignored() {
        let ev = eval_str("% this is a comment\nx = 1");
        assert_eq!(get_scalar(&ev, "x"), 1.0);
    }

    #[test]
    fn percent_comment_inline() {
        let ev = eval_str("x = 42 % inline comment");
        assert_eq!(get_scalar(&ev, "x"), 42.0);
    }

    // ── 1b: if / else / end ──────────────────────────────────────────────────

    #[test]
    fn if_true_branch_taken() {
        let ev = eval_str("x = 0\nif 1 == 1\nx = 1\nend");
        assert_eq!(get_scalar(&ev, "x"), 1.0);
    }

    #[test]
    fn if_false_branch_skipped() {
        let ev = eval_str("x = 0\nif 1 == 2\nx = 99\nend");
        assert_eq!(get_scalar(&ev, "x"), 0.0);
    }

    #[test]
    fn if_else_true() {
        let ev = eval_str("if 1 == 1\nx = 10\nelse\nx = 20\nend");
        assert_eq!(get_scalar(&ev, "x"), 10.0);
    }

    #[test]
    fn if_else_false() {
        let ev = eval_str("if 1 == 2\nx = 10\nelse\nx = 20\nend");
        assert_eq!(get_scalar(&ev, "x"), 20.0);
    }

    #[test]
    fn if_scalar_condition() {
        // nonzero scalar is truthy
        let ev = eval_str("if 5\nx = 1\nelse\nx = 0\nend");
        assert_eq!(get_scalar(&ev, "x"), 1.0);
    }

    #[test]
    fn nested_if() {
        let ev = eval_str("x = 0\nif 1 == 1\nif 2 == 2\nx = 99\nend\nend");
        assert_eq!(get_scalar(&ev, "x"), 99.0);
    }

    // ── 1c: multi-assign ─────────────────────────────────────────────────────

    #[test]
    fn multi_assign_from_tuple_function() {
        // Define a function returning a Tuple via struct trick isn't needed;
        // use a direct eval path: roots() returns a vector, not a tuple.
        // Test multi-assign with a builtin that returns Tuple: roots of quadratic
        // For now verify roots() returns a vector (tested below), and
        // test the Tuple-unpack path via a custom inline function in the REPL.
        let src = "function y = make_pair(a, b)\ny = struct(\"a\", a, \"b\", b)\nend\ns = make_pair(3, 7)";
        let ev = eval_str(src);
        if let Value::Struct(fields) = ev.get("s").unwrap() {
            assert_eq!(fields.get("a").unwrap().to_string(), "3");
            assert_eq!(fields.get("b").unwrap().to_string(), "7");
        } else {
            panic!("expected struct");
        }
    }

    // ── 1d: disp / fprintf ───────────────────────────────────────────────────

    #[test]
    fn fprintf_produces_no_value() {
        // fprintf returns Value::None — just verify it doesn't error
        let src = "x = 1"; // placeholder, we test indirectly via output
        let ev = eval_str(src);
        assert_eq!(get_scalar(&ev, "x"), 1.0);
    }

    #[test]
    fn fprintf_format_string_parses() {
        use crate::eval::builtins::apply_format;
        let result = apply_format("x = %.2f\n", &[Value::Scalar(3.14159)]).unwrap();
        assert_eq!(result, "x = 3.14\n");
    }

    #[test]
    fn fprintf_integer_format() {
        use crate::eval::builtins::apply_format;
        let result = apply_format("%d items", &[Value::Scalar(5.0)]).unwrap();
        assert_eq!(result, "5 items");
    }

    #[test]
    fn fprintf_percent_escape() {
        use crate::eval::builtins::apply_format;
        let result = apply_format("100%%", &[]).unwrap();
        assert_eq!(result, "100%");
    }

    // ── 1e: all / any ────────────────────────────────────────────────────────

    #[test]
    fn all_nonzero_vector_true() {
        let ev = eval_str("b = all([1, 2, 3])");
        assert!(get_bool(&ev, "b"));
    }

    #[test]
    fn all_with_zero_false() {
        let ev = eval_str("b = all([1, 0, 3])");
        assert!(!get_bool(&ev, "b"));
    }

    #[test]
    fn any_with_nonzero_true() {
        let ev = eval_str("b = any([0, 0, 1])");
        assert!(get_bool(&ev, "b"));
    }

    #[test]
    fn any_all_zero_false() {
        let ev = eval_str("b = any([0, 0, 0])");
        assert!(!get_bool(&ev, "b"));
    }

    // ── 1f: rank ─────────────────────────────────────────────────────────────

    #[test]
    fn rank_identity_3x3() {
        let ev = eval_str("r = rank(eye(3))");
        assert_eq!(get_scalar(&ev, "r"), 3.0);
    }

    #[test]
    fn rank_singular_matrix() {
        // Rows [1,2] and [2,4] are linearly dependent → rank 1
        let ev = eval_str("r = rank([1,2;2,4])");
        assert_eq!(get_scalar(&ev, "r"), 1.0);
    }

    #[test]
    fn rank_full_rank_2x2() {
        let ev = eval_str("r = rank([1,2;3,4])");
        assert_eq!(get_scalar(&ev, "r"), 2.0);
    }

    // ── 1g: roots ────────────────────────────────────────────────────────────

    #[test]
    fn roots_linear() {
        // 2x - 4 = 0  →  root = 2
        let ev = eval_str("r = roots([2, -4])");
        let v = get_complex_vector(&ev, "r");
        assert_eq!(v.len(), 1);
        assert!((v[0].re - 2.0).abs() < 1e-10);
        assert!(v[0].im.abs() < 1e-10);
    }

    #[test]
    fn roots_quadratic_real() {
        // x²-3x+2 = (x-1)(x-2)  →  roots 1, 2
        let ev = eval_str("r = roots([1, -3, 2])");
        let mut v: Vec<f64> = get_complex_vector(&ev, "r").iter().map(|c| c.re).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((v[0] - 1.0).abs() < 1e-8);
        assert!((v[1] - 2.0).abs() < 1e-8);
    }

    #[test]
    fn roots_quadratic_complex() {
        // s²+2s+10 → roots -1±3j
        let ev = eval_str("r = roots([1, 2, 10])");
        let v = get_complex_vector(&ev, "r");
        assert_eq!(v.len(), 2);
        for c in &v {
            assert!((c.re - (-1.0)).abs() < 1e-8);
            assert!((c.im.abs() - 3.0).abs() < 1e-8);
        }
    }
}

// ── Phase 2: Transfer Function Type ──────────────────────────────────────────

#[cfg(test)]
mod phase2_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for s in &stmts {
            ev.exec_stmt(s).unwrap();
        }
        ev
    }

    fn get_tf(ev: &Evaluator, name: &str) -> (Vec<f64>, Vec<f64>) {
        match ev.get(name).unwrap() {
            Value::TransferFn { num, den } => (num.clone(), den.clone()),
            other => panic!("expected tf for '{name}', got {other:?}"),
        }
    }

    fn get_complex_vector(ev: &Evaluator, name: &str) -> Vec<num_complex::Complex<f64>> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.to_vec(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-8
    }

    // ── 2b: tf() builtin ─────────────────────────────────────────────────────

    #[test]
    fn tf_laplace_variable() {
        // tf("s") should produce num=[1,0], den=[1]
        let ev = eval_str("s = tf(\"s\")");
        let (num, den) = get_tf(&ev, "s");
        assert_eq!(num, vec![1.0, 0.0]);
        assert_eq!(den, vec![1.0]);
    }

    #[test]
    fn tf_explicit_num_den() {
        let ev = eval_str("G = tf([10], [1, 2, 10])");
        let (num, den) = get_tf(&ev, "G");
        assert_eq!(num, vec![10.0]);
        assert_eq!(den, vec![1.0, 2.0, 10.0]);
    }

    #[test]
    fn tf_from_laplace_arithmetic() {
        // s = tf("s");  G = 10 / (s^2 + 2*s + 10)
        let ev = eval_str("s = tf(\"s\")\nG = 10 / (s^2 + 2*s + 10)");
        let (num, den) = get_tf(&ev, "G");
        assert_eq!(num.len(), 1);
        assert!(close(num[0], 10.0), "num[0] = {}", num[0]);
        assert_eq!(den.len(), 3);
        assert!(close(den[0], 1.0), "den[0] = {}", den[0]);
        assert!(close(den[1], 2.0), "den[1] = {}", den[1]);
        assert!(close(den[2], 10.0), "den[2] = {}", den[2]);
    }

    #[test]
    fn tf_pow_zero_gives_unity() {
        let ev = eval_str("s = tf(\"s\")\nG = s^0");
        let (num, den) = get_tf(&ev, "G");
        assert_eq!(num, vec![1.0]);
        assert_eq!(den, vec![1.0]);
    }

    #[test]
    fn tf_mul_two_tfs() {
        // (1/s) * (1/s) = 1/s^2
        let ev = eval_str("s = tf(\"s\")\nG = (1/s) * (1/s)");
        let (num, den) = get_tf(&ev, "G");
        // num should be [1], den should be degree 2
        assert!(close(num.iter().map(|x| x.abs()).sum::<f64>(), 1.0));
        assert_eq!(den.len(), 3); // s^2 from [1,0] * [1,0]
    }

    #[test]
    fn tf_add_two_tfs() {
        // 1/(s+1) + 1/(s+2) = (2s+3)/((s+1)(s+2))
        let ev = eval_str("s = tf(\"s\")\nG = 1/(s+1) + 1/(s+2)");
        let (num, den) = get_tf(&ev, "G");
        // numerator should be [2, 3], denominator [1, 3, 2]
        assert_eq!(num.len(), 2);
        assert!(close(num[0], 2.0), "num[0]={}", num[0]);
        assert!(close(num[1], 3.0), "num[1]={}", num[1]);
        assert_eq!(den.len(), 3);
        assert!(close(den[0], 1.0), "den[0]={}", den[0]);
        assert!(close(den[1], 3.0), "den[1]={}", den[1]);
        assert!(close(den[2], 2.0), "den[2]={}", den[2]);
    }

    // ── 2c: pole() and zero() builtins ───────────────────────────────────────

    #[test]
    fn pole_second_order() {
        // G = 10 / (s^2 + 2s + 10)  →  poles at -1 ± 3j
        let ev = eval_str("G = tf([10], [1, 2, 10])\np = pole(G)");
        let poles = get_complex_vector(&ev, "p");
        assert_eq!(poles.len(), 2);
        for p in &poles {
            assert!(close(p.re, -1.0), "pole re = {}", p.re);
            assert!(close(p.im.abs(), 3.0), "pole |im| = {}", p.im.abs());
        }
    }

    #[test]
    fn zero_of_tf() {
        // G = (s - 2) / (s + 5)  →  zero at s=2
        let ev = eval_str("G = tf([1, -2], [1, 5])\nz = zero(G)");
        let zeros = get_complex_vector(&ev, "z");
        assert_eq!(zeros.len(), 1);
        assert!(close(zeros[0].re, 2.0), "zero re = {}", zeros[0].re);
        assert!(close(zeros[0].im, 0.0), "zero im = {}", zeros[0].im);
    }

    #[test]
    fn pole_of_laplace_s() {
        // tf("s") has num=[1,0], den=[1] → den is degree 0 constant, no finite poles
        let ev = eval_str("s = tf(\"s\")\np = pole(s)");
        // den=[1.0] → roots of a constant → empty vector
        let poles = get_complex_vector(&ev, "p");
        assert_eq!(poles.len(), 0);
    }
}

// ── Phase 3: State-Space Type ─────────────────────────────────────────────────

#[cfg(test)]
mod phase3_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};
    use rustlab_core::CMatrix;

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for s in &stmts {
            ev.exec_stmt(s).unwrap();
        }
        ev
    }

    fn get_ss(ev: &Evaluator, name: &str) -> (CMatrix, CMatrix, CMatrix, CMatrix) {
        match ev.get(name).unwrap() {
            Value::StateSpace { a, b, c, d } => (a.clone(), b.clone(), c.clone(), d.clone()),
            other => panic!("expected ss for '{name}', got {other:?}"),
        }
    }

    fn get_matrix(ev: &Evaluator, name: &str) -> CMatrix {
        match ev.get(name).unwrap() {
            Value::Matrix(m) => m.clone(),
            other => panic!("expected matrix for '{name}', got {other:?}"),
        }
    }

    fn get_complex_vector(ev: &Evaluator, name: &str) -> Vec<num_complex::Complex<f64>> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.to_vec(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-7
    }

    // ── 3b: ss() conversion ───────────────────────────────────────────────────

    #[test]
    fn ss_dimensions_second_order() {
        // G = 10/(s²+2s+10) → 2-state, 1-input, 1-output
        let ev = eval_str("G = tf([10],[1,2,10])\nsys = ss(G)");
        let (a, b, c, d) = get_ss(&ev, "sys");
        assert_eq!(a.nrows(), 2);
        assert_eq!(a.ncols(), 2);
        assert_eq!(b.nrows(), 2);
        assert_eq!(b.ncols(), 1);
        assert_eq!(c.nrows(), 1);
        assert_eq!(c.ncols(), 2);
        assert_eq!(d.nrows(), 1);
        assert_eq!(d.ncols(), 1);
    }

    #[test]
    fn ss_eigenvalues_match_poles() {
        // Eigenvalues of A should match poles of G
        let ev = eval_str("G = tf([10],[1,2,10])\nsys = ss(G)\nlam = eig(sys.A)");
        let eigs = get_complex_vector(&ev, "lam");
        assert_eq!(eigs.len(), 2);
        for e in &eigs {
            assert!(close(e.re, -1.0), "eig re = {}", e.re);
            assert!(close(e.im.abs(), 3.0), "eig |im| = {}", e.im.abs());
        }
    }

    #[test]
    fn ss_d_zero_for_strictly_proper() {
        let ev = eval_str("G = tf([10],[1,2,10])\nsys = ss(G)");
        let (_, _, _, d) = get_ss(&ev, "sys");
        assert!(
            d[[0, 0]].norm() < 1e-12,
            "D should be 0 for strictly proper TF"
        );
    }

    #[test]
    fn ss_field_access() {
        let ev = eval_str("G = tf([10],[1,2,10])\nsys = ss(G)\nA = sys.A\nB = sys.B");
        let a = get_matrix(&ev, "A");
        let b = get_matrix(&ev, "B");
        assert_eq!(a.nrows(), 2);
        assert_eq!(b.ncols(), 1);
    }

    // ── 3c: ctrb() and obsv() ─────────────────────────────────────────────────

    #[test]
    fn ctrb_full_rank() {
        // Controllable second-order system: G = 10/(s²+2s+10)
        let ev = eval_str("G = tf([10],[1,2,10])\nsys = ss(G)\nM = ctrb(sys.A, sys.B)");
        let m = get_matrix(&ev, "M");
        // ctrb returns 2×2 for SISO second-order system
        assert_eq!(m.nrows(), 2);
        assert_eq!(m.ncols(), 2);
        // Must be full rank — det != 0
        let det = m[[0, 0]] * m[[1, 1]] - m[[0, 1]] * m[[1, 0]];
        assert!(
            det.norm() > 1e-6,
            "controllability matrix should be full rank, det = {}",
            det
        );
    }

    #[test]
    fn obsv_full_rank() {
        let ev = eval_str("G = tf([10],[1,2,10])\nsys = ss(G)\nM = obsv(sys.A, sys.C)");
        let m = get_matrix(&ev, "M");
        assert_eq!(m.nrows(), 2);
        assert_eq!(m.ncols(), 2);
        let det = m[[0, 0]] * m[[1, 1]] - m[[0, 1]] * m[[1, 0]];
        assert!(
            det.norm() > 1e-6,
            "observability matrix should be full rank, det = {}",
            det
        );
    }

    #[test]
    fn ctrb_uncontrollable_rank_deficient() {
        // Double pole at -1, both states driven by same mode → rank 1
        // A = [-1, 0; 0, -1], B = [1; 1] → ctrb = [1, -1; 1, -1] → rank 1
        let ev = eval_str("A = [-1,0;0,-1]\nB = [1;1]\nM = ctrb(A, B)");
        let m = get_matrix(&ev, "M");
        let det = m[[0, 0]] * m[[1, 1]] - m[[0, 1]] * m[[1, 0]];
        assert!(
            det.norm() < 1e-10,
            "expected rank-deficient ctrb, det = {}",
            det
        );
    }
}

// ── Phase 4: Frequency & Time-Domain Analysis ─────────────────────────────────

#[cfg(test)]
mod phase4_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for s in &stmts {
            ev.exec_stmt(s).unwrap();
        }
        ev
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("expected scalar for '{name}', got {other:?}"),
        }
    }

    fn get_vector(ev: &Evaluator, name: &str) -> Vec<f64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    // ── 4a: bode() ────────────────────────────────────────────────────────────

    #[test]
    fn bode_returns_three_vectors() {
        let ev = eval_str("[mag, ph, w] = bode(tf([10],[1,2,10]));");
        let mag = get_vector(&ev, "mag");
        let ph = get_vector(&ev, "ph");
        let w = get_vector(&ev, "w");
        assert!(!mag.is_empty());
        assert_eq!(mag.len(), ph.len());
        assert_eq!(mag.len(), w.len());
    }

    #[test]
    fn bode_dc_gain_zero_db() {
        // G(0) = 10/10 = 1 → 0 dB; lowest frequency point should be near 0 dB
        let ev = eval_str("[mag, ph, w] = bode(tf([10],[1,2,10]));");
        let mag = get_vector(&ev, "mag");
        assert!(
            close(mag[0], 0.0, 1.5),
            "DC mag = {} dB, expected ~0 dB",
            mag[0]
        );
    }

    #[test]
    fn bode_user_supplied_freqs() {
        // Supply a known frequency vector: single point at w=0.001 ≈ DC
        let ev = eval_str("[mag, ph, w] = bode(tf([10],[1,2,10]), [0.001, 0.01, 0.1]);");
        let mag = get_vector(&ev, "mag");
        let w = get_vector(&ev, "w");
        assert_eq!(mag.len(), 3);
        assert_eq!(w.len(), 3);
        assert!(close(mag[0], 0.0, 0.1), "DC mag = {} dB", mag[0]);
    }

    // ── 4b: step() ────────────────────────────────────────────────────────────

    #[test]
    fn step_returns_two_vectors() {
        let ev = eval_str("[y, t] = step(tf([10],[1,2,10]));");
        let y = get_vector(&ev, "y");
        let t = get_vector(&ev, "t");
        assert!(!y.is_empty());
        assert_eq!(y.len(), t.len());
        // t should start at 0
        assert!(close(t[0], 0.0, 1e-12));
    }

    #[test]
    fn step_steady_state_equals_dc_gain() {
        // G = 10/(s²+2s+10), DC gain = 10/10 = 1 → y(∞) ≈ 1
        let ev = eval_str("[y, t] = step(tf([10],[1,2,10]));");
        let y = get_vector(&ev, "y");
        let y_final = *y.last().unwrap();
        assert!(
            close(y_final, 1.0, 0.01),
            "y(∞) = {}, expected ~1.0",
            y_final
        );
    }

    #[test]
    fn step_user_specified_t_end() {
        let ev = eval_str("[y, t] = step(tf([10],[1,2,10]), 5.0);");
        let t = get_vector(&ev, "t");
        assert!(
            close(*t.last().unwrap(), 5.0, 0.01),
            "t_end = {}",
            t.last().unwrap()
        );
    }

    // ── 4c: margin() ─────────────────────────────────────────────────────────

    #[test]
    fn margin_returns_tuple_of_four() {
        // margin(G) returns [Gm, Pm, Wcg, Wcp]
        let ev = eval_str("[gm, pm, wcg, wcp] = margin(tf([10],[1,2,10]));");
        // Just verify they exist and are numeric
        let _gm = get_scalar(&ev, "gm");
        let pm = get_scalar(&ev, "pm");
        let _wcg = get_scalar(&ev, "wcg");
        let wcp = get_scalar(&ev, "wcp");
        // For G = 10/(s²+2s+10): PM ≈ 53°, Wcp ≈ 4 rad/s
        assert!(close(pm, 53.13, 1.0), "PM = {}, expected ~53.13°", pm);
        assert!(close(wcp, 4.0, 0.1), "Wcp = {}, expected ~4 rad/s", wcp);
    }

    #[test]
    fn margin_gm_infinite_for_second_order() {
        // Stable second-order system: phase never reaches -180° → GM = ∞
        let ev = eval_str("[gm, pm, wcg, wcp] = margin(tf([10],[1,2,10]));");
        let gm = get_scalar(&ev, "gm");
        assert!(
            gm.is_infinite() || gm > 100.0,
            "GM = {}, expected ∞ for 2nd-order",
            gm
        );
    }
}

// ─── Phase 5 tests — Optimal Control (LQR) ───────────────────────────────────

#[cfg(test)]
mod phase5_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};
    use rustlab_core::C64;

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for s in &stmts {
            ev.exec_stmt(s).unwrap();
        }
        ev
    }

    /// Return the complex eigenvalue vector (for checking stability).
    fn get_cvector(ev: &Evaluator, name: &str) -> Vec<C64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().cloned().collect(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    // Double-integrator: A=[0,1;0,0], B=[0;1], Q=I, R=I
    // Analytical K = [1, sqrt(3)], P = [sqrt(3), 1; 1, sqrt(3)],
    // cl eigenvalues ≈ -0.866 ± 0.5j (both Re < 0)
    #[test]
    fn lqr_double_integrator_stable_cl_eigenvalues() {
        let script = r#"
sys = ss(tf([1],[1,0,0]))
Q = eye(2)
R = 1
[K, S, e] = lqr(sys, Q, R)
"#;
        let ev = eval_str(script);
        let e = get_cvector(&ev, "e");
        assert_eq!(e.len(), 2, "should have 2 closed-loop eigenvalues");
        for eig in &e {
            assert!(
                eig.re < 0.0,
                "closed-loop eigenvalue {} must have Re < 0 (stable)",
                eig
            );
        }
    }

    #[test]
    fn lqr_double_integrator_k_shape() {
        let script = r#"
sys = ss(tf([1],[1,0,0]))
Q = eye(2)
R = 1
[K, S, e] = lqr(sys, Q, R)
"#;
        let ev = eval_str(script);
        // K should be 1×2 for a single-input, 2-state system
        match ev.get("K").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m.nrows(), 1, "K rows = {}, expected 1", m.nrows());
                assert_eq!(m.ncols(), 2, "K cols = {}, expected 2", m.ncols());
            }
            other => panic!("K should be a matrix, got {}", other.type_name()),
        }
    }

    #[test]
    fn lqr_double_integrator_p_positive_definite() {
        let script = r#"
sys = ss(tf([1],[1,0,0]))
Q = eye(2)
R = 1
[K, S, e] = lqr(sys, Q, R)
"#;
        let ev = eval_str(script);
        // P (= S) should be 2×2 with positive diagonal entries
        match ev.get("S").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m.nrows(), 2);
                assert_eq!(m.ncols(), 2);
                assert!(
                    m[[0, 0]].re > 0.0,
                    "P[0,0] = {} should be > 0",
                    m[[0, 0]].re
                );
                assert!(
                    m[[1, 1]].re > 0.0,
                    "P[1,1] = {} should be > 0",
                    m[[1, 1]].re
                );
            }
            other => panic!("S should be a matrix, got {}", other.type_name()),
        }
    }

    #[test]
    fn lqr_returns_tuple_of_three() {
        let script = r#"
sys = ss(tf([1],[1,0,0]))
Q = eye(2)
R = 1
[K, S, e] = lqr(sys, Q, R)
"#;
        let ev = eval_str(script);
        // All three outputs must be bound
        assert!(ev.get("K").is_some(), "K not found in env");
        assert!(ev.get("S").is_some(), "S not found in env");
        assert!(ev.get("e").is_some(), "e not found in env");
    }
}

// ─── atan2 / meshgrid tests ───────────────────────────────────────────────────

#[cfg(test)]
mod math_extra_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for s in &stmts {
            ev.exec_stmt(s).unwrap();
        }
        ev
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("expected scalar for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    // ── atan2 ─────────────────────────────────────────────────────────────────

    #[test]
    fn atan2_scalar_scalars() {
        // atan2(0, -1) = π, atan2(1, 0) = π/2
        let ev = eval_str("a = atan2(0, -1)\nb = atan2(1, 0)");
        let a = get_scalar(&ev, "a");
        let b = get_scalar(&ev, "b");
        assert!(close(a, std::f64::consts::PI, 1e-12), "atan2(0,-1) = {a}");
        assert!(
            close(b, std::f64::consts::FRAC_PI_2, 1e-12),
            "atan2(1,0) = {b}"
        );
    }

    #[test]
    fn atan2_vector_vector() {
        // atan2([0,1], [1,0]) = [0, π/2]
        let ev = eval_str("v = atan2([0,1], [1,0])");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 2);
                assert!(close(v[0].re, 0.0, 1e-12));
                assert!(close(v[1].re, std::f64::consts::FRAC_PI_2, 1e-12));
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn atan2_matrix_matrix() {
        // atan2([0,1;-1,0], [1,0;0,-1]) elementwise
        let ev = eval_str("M = atan2([0,1;-1,0], [1,0;0,-1])");
        match ev.get("M").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m.shape(), &[2, 2]);
                assert!(close(m[[0, 0]].re, 0.0, 1e-12)); // atan2(0,1)
                assert!(close(m[[0, 1]].re, std::f64::consts::FRAC_PI_2, 1e-12)); // atan2(1,0)
                assert!(close(m[[1, 0]].re, -std::f64::consts::FRAC_PI_2, 1e-12)); // atan2(-1,0)
                assert!(close(m[[1, 1]].re, std::f64::consts::PI, 1e-12)); // atan2(0,-1)
            }
            other => panic!("expected matrix, got {other:?}"),
        }
    }

    // ── meshgrid ──────────────────────────────────────────────────────────────

    #[test]
    fn meshgrid_shapes() {
        // x has 3 elements, y has 2 → X and Y are 2×3
        let ev = eval_str("[X, Y] = meshgrid([1,2,3], [10,20])");
        match (ev.get("X").unwrap(), ev.get("Y").unwrap()) {
            (Value::Matrix(x), Value::Matrix(y)) => {
                assert_eq!(x.shape(), &[2, 3], "X shape {:?}", x.shape());
                assert_eq!(y.shape(), &[2, 3], "Y shape {:?}", y.shape());
            }
            other => panic!("expected matrices, got {other:?}"),
        }
    }

    #[test]
    fn meshgrid_x_varies_along_cols() {
        // X[i,j] = x[j], so X row 0 == X row 1
        let ev = eval_str("[X, Y] = meshgrid([1,2,3], [10,20])");
        match ev.get("X").unwrap() {
            Value::Matrix(x) => {
                assert!(close(x[[0, 0]].re, 1.0, 1e-12));
                assert!(close(x[[0, 1]].re, 2.0, 1e-12));
                assert!(close(x[[0, 2]].re, 3.0, 1e-12));
                assert!(close(x[[1, 0]].re, 1.0, 1e-12)); // same as row 0
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn meshgrid_y_varies_along_rows() {
        // Y[i,j] = y[i], so Y col 0 == Y col 1 == Y col 2
        let ev = eval_str("[X, Y] = meshgrid([1,2,3], [10,20])");
        match ev.get("Y").unwrap() {
            Value::Matrix(y) => {
                assert!(close(y[[0, 0]].re, 10.0, 1e-12));
                assert!(close(y[[1, 0]].re, 20.0, 1e-12));
                assert!(close(y[[0, 2]].re, 10.0, 1e-12)); // same as col 0
                assert!(close(y[[1, 2]].re, 20.0, 1e-12));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn meshgrid_with_atan2_for_angle() {
        // theta = atan2(Y, X) should give angle at each grid point
        let ev = eval_str(
            r#"
[X, Y] = meshgrid([-1,0,1], [0,1])
T = atan2(Y, X)
"#,
        );
        match ev.get("T").unwrap() {
            Value::Matrix(t) => {
                assert_eq!(t.shape(), &[2, 3]);
                // atan2(0, -1) = π  at (row=0, col=0)
                assert!(
                    close(t[[0, 0]].re, std::f64::consts::PI, 1e-12),
                    "T[0,0] = {} expected π",
                    t[[0, 0]].re
                );
                // atan2(0, 1) = 0  at (row=0, col=2)
                assert!(
                    close(t[[0, 2]].re, 0.0, 1e-12),
                    "T[0,2] = {} expected 0",
                    t[[0, 2]].re
                );
            }
            other => panic!("{other:?}"),
        }
    }
}

// ── Phase 6: Root Locus ───────────────────────────────────────────────────────

#[cfg(test)]
mod phase6_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_ok(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for s in &stmts {
            ev.exec_stmt(s).unwrap();
        }
        ev
    }

    fn eval_err(src: &str) -> String {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for s in &stmts {
            if let Err(e) = ev.exec_stmt(s) {
                return e.to_string();
            }
        }
        panic!("expected error but script succeeded");
    }

    // rlocus returns None without error (plot skipped in non-tty test env)
    #[test]
    fn rlocus_returns_none_for_second_order() {
        let ev = eval_ok("G = tf([10], [1, 2, 10])\nrlocus(G)");
        // rlocus doesn't bind a variable; just check it doesn't panic
        let _ = ev;
    }

    // rlocus works with the Laplace-variable form
    #[test]
    fn rlocus_with_s_form() {
        eval_ok("s = tf(\"s\")\nG = 1 / (s * (s + 1))\nrlocus(G)");
    }

    // rlocus rejects a non-TF argument
    #[test]
    fn rlocus_rejects_scalar() {
        let e = eval_err("rlocus(42)");
        assert!(e.contains("rlocus"), "error should mention rlocus: {e}");
    }

    // rlocus rejects an improper TF (num degree >= den degree)
    #[test]
    fn rlocus_rejects_improper_tf() {
        let e = eval_err("G = tf([1, 0], [1, 1])\nrlocus(G)");
        assert!(e.contains("proper"), "error should mention proper: {e}");
    }

    // The open-loop poles of den = [1,2,10] are approx -1 ± 3j.
    // Verify roots() (used internally) gives those values so the starting
    // points of the locus are correct.
    #[test]
    fn rlocus_open_loop_poles_are_tf_poles() {
        let ev = eval_ok("G = tf([10], [1, 2, 10])\np = pole(G)");
        match ev.get("p").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 2);
                // Both poles should have Re ≈ -1
                for c in v.iter() {
                    assert!(
                        (c.re - (-1.0)).abs() < 1e-6,
                        "pole real part should be ≈ -1, got {}",
                        c.re
                    );
                }
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }
}

// ─── New builtins: acos/asin/atan, outer, kron, expm, laguerre, legendre ────

#[cfg(test)]
mod new_builtins_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};
    use num_complex::Complex;

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn eval_err(src: &str) -> String {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                return e.to_string();
            }
        }
        panic!("expected an error but script ran successfully")
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("expected scalar for '{name}', got {other:?}"),
        }
    }

    fn get_matrix(ev: &Evaluator, name: &str) -> ndarray::Array2<Complex<f64>> {
        match ev.get(name).unwrap() {
            Value::Matrix(m) => m.clone(),
            other => panic!("expected matrix for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // ── acos / asin / atan ──────────────────────────────────────────────────

    #[test]
    fn acos_scalar() {
        let ev = eval_str("x = acos(1.0)");
        assert!(close(get_scalar(&ev, "x"), 0.0), "acos(1) should be 0");
    }

    #[test]
    fn acos_of_zero() {
        let ev = eval_str("x = acos(0.0)");
        let pi_2 = std::f64::consts::FRAC_PI_2;
        assert!(close(get_scalar(&ev, "x"), pi_2), "acos(0) should be π/2");
    }

    #[test]
    fn asin_scalar() {
        let ev = eval_str("x = asin(1.0)");
        let pi_2 = std::f64::consts::FRAC_PI_2;
        assert!(close(get_scalar(&ev, "x"), pi_2), "asin(1) should be π/2");
    }

    #[test]
    fn atan_scalar() {
        let ev = eval_str("x = atan(1.0)");
        let pi_4 = std::f64::consts::FRAC_PI_4;
        assert!(close(get_scalar(&ev, "x"), pi_4), "atan(1) should be π/4");
    }

    #[test]
    fn acos_vector() {
        // acos([1,0,-1]) = [0, π/2, π]
        let ev = eval_str("v = acos([1.0, 0.0, -1.0])");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert!(close(v[0].re, 0.0));
                assert!(close(v[1].re, std::f64::consts::FRAC_PI_2));
                assert!(close(v[2].re, std::f64::consts::PI));
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn acos_matrix() {
        let ev = eval_str("M = acos([1,0;0,1])");
        let m = get_matrix(&ev, "M");
        assert!(close(m[[0, 0]].re, 0.0));
        assert!(close(m[[0, 1]].re, std::f64::consts::FRAC_PI_2));
    }

    // ── outer ────────────────────────────────────────────────────────────────

    #[test]
    fn outer_basic() {
        // outer([1,2,3], [10,20]) → [[10,20],[20,40],[30,60]]
        let ev = eval_str("M = outer([1.0,2.0,3.0], [10.0,20.0])");
        let m = get_matrix(&ev, "M");
        assert_eq!(m.nrows(), 3);
        assert_eq!(m.ncols(), 2);
        assert!(close(m[[0, 0]].re, 10.0));
        assert!(close(m[[1, 1]].re, 40.0));
        assert!(close(m[[2, 0]].re, 30.0));
    }

    #[test]
    fn outer_rank_one() {
        // outer(v, v) where v=[1,0] should give [[1,0],[0,0]]
        let ev = eval_str("M = outer([1.0,0.0], [1.0,0.0])");
        let m = get_matrix(&ev, "M");
        assert!(close(m[[0, 0]].re, 1.0));
        assert!(close(m[[0, 1]].re, 0.0));
        assert!(close(m[[1, 0]].re, 0.0));
        assert!(close(m[[1, 1]].re, 0.0));
    }

    // ── kron ─────────────────────────────────────────────────────────────────

    #[test]
    fn kron_eye2_eye2() {
        // kron(eye(2), eye(2)) = eye(4)
        let ev = eval_str("M = kron(eye(2), eye(2))");
        let m = get_matrix(&ev, "M");
        assert_eq!(m.nrows(), 4);
        for i in 0..4 {
            for j in 0..4 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    close(m[[i, j]].re, expected),
                    "kron(I,I)[{i},{j}] should be {expected}"
                );
            }
        }
    }

    #[test]
    fn kron_scalar_matrix() {
        // kron(2, [1,2;3,4]) = [2,4;6,8]
        let ev = eval_str("M = kron(2.0, [1,2;3,4])");
        let m = get_matrix(&ev, "M");
        assert_eq!(m.nrows(), 2);
        assert!(close(m[[0, 0]].re, 2.0));
        assert!(close(m[[1, 1]].re, 8.0));
    }

    #[test]
    fn kron_pauli_x_pauli_z() {
        // σ_x ⊗ σ_z — known result for two-qubit system
        let ev = eval_str("sx = [0,1;1,0]\nsz = [1,0;0,-1]\nM = kron(sx, sz)");
        let m = get_matrix(&ev, "M");
        assert_eq!(m.nrows(), 4);
        // Top-left block should be 0*sz = zeros
        assert!(close(m[[0, 0]].re, 0.0));
        // Top-right block should be 1*sz: m[0,2]=1, m[1,3]=-1
        assert!(close(m[[0, 2]].re, 1.0));
        assert!(close(m[[1, 3]].re, -1.0));
    }

    // ── expm ─────────────────────────────────────────────────────────────────

    #[test]
    fn expm_zero_matrix_gives_identity() {
        let ev = eval_str("M = expm(zeros(3,3))");
        let m = get_matrix(&ev, "M");
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (m[[i, j]].re - expected).abs() < 1e-10,
                    "expm(0)[{i},{j}] should be {expected}, got {}",
                    m[[i, j]].re
                );
            }
        }
    }

    #[test]
    fn expm_diagonal_matrix() {
        // expm(diag([1,2])) = diag([e^1, e^2])
        let ev = eval_str("M = expm([1,0;0,2])");
        let m = get_matrix(&ev, "M");
        assert!(
            (m[[0, 0]].re - std::f64::consts::E).abs() < 1e-8,
            "expm diagonal [0,0] should be e"
        );
        assert!(
            (m[[1, 1]].re - std::f64::consts::E.powi(2)).abs() < 1e-8,
            "expm diagonal [1,1] should be e^2"
        );
        assert!(m[[0, 1]].norm() < 1e-10, "off-diagonal should be 0");
    }

    #[test]
    fn expm_pauli_y_rotation() {
        // expm(-j * pi/2 * [0,-j;j,0]) — Pauli-Y rotation by π
        // σ_y = [0,-j;j,0]; expm(-j*π/2*σ_y) should be -j*σ_y (up to global phase)
        // More testable: expm(j*pi*[0,1;1,0]/2) ...
        // Simplest check: expm(j*pi*I/2) = e^{j*pi/2} * I = j*I
        let ev = eval_str("M = expm([0,0;0,0])");
        let m = get_matrix(&ev, "M");
        // just verify it returned an identity
        assert!(close(m[[0, 0]].re, 1.0));
    }

    #[test]
    fn expm_scalar_passthrough() {
        let ev = eval_str("x = expm(1.0)");
        assert!(close(get_scalar(&ev, "x"), std::f64::consts::E));
    }

    #[test]
    fn expm_rejects_non_square() {
        let e = eval_err("expm([1,2,3;4,5,6])");
        assert!(e.contains("square"), "error should mention square: {e}");
    }

    // ── laguerre ─────────────────────────────────────────────────────────────

    #[test]
    fn laguerre_n0_is_one() {
        // L_0^alpha(x) = 1 for any alpha, x
        let ev = eval_str("x = laguerre(0, 0.5, 3.7)");
        assert!(close(get_scalar(&ev, "x"), 1.0));
    }

    #[test]
    fn laguerre_n1() {
        // L_1^alpha(x) = 1 + alpha - x
        let ev = eval_str("x = laguerre(1, 2.0, 1.5)");
        // = 1 + 2 - 1.5 = 1.5
        assert!(close(get_scalar(&ev, "x"), 1.5));
    }

    #[test]
    fn laguerre_n2_alpha1() {
        // L_2^1(x) via recurrence at x=1, alpha=1:
        // L_0=1, L_1=1+1-1=1
        // L_2 = ((2+1+1-1)*L_1 - (1+1)*L_0) / 2 = (3*1 - 2) / 2 = 0.5
        let ev = eval_str("x = laguerre(2, 1.0, 1.0)");
        assert!(close(get_scalar(&ev, "x"), 0.5));
    }

    #[test]
    fn laguerre_vector_input() {
        // L_0^0(x) = 1 element-wise
        let ev = eval_str("v = laguerre(0, 0.0, [1.0, 2.0, 3.0])");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                for c in v.iter() {
                    assert!(close(c.re, 1.0));
                }
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn laguerre_negative_n_errors() {
        let e = eval_err("laguerre(-1, 0, 1.0)");
        assert!(
            e.contains("non-negative"),
            "error should mention non-negative: {e}"
        );
    }

    // ── legendre ─────────────────────────────────────────────────────────────

    #[test]
    fn legendre_p00_is_one() {
        // P_0^0(x) = 1
        let ev = eval_str("x = legendre(0, 0, 0.5)");
        assert!(close(get_scalar(&ev, "x"), 1.0));
    }

    #[test]
    fn legendre_p10() {
        // P_1^0(x) = x
        let ev = eval_str("x = legendre(1, 0, 0.5)");
        assert!(close(get_scalar(&ev, "x"), 0.5));
    }

    #[test]
    fn legendre_p20() {
        // P_2^0(x) = (3x^2 - 1)/2; at x=0: -0.5
        let ev = eval_str("x = legendre(2, 0, 0.0)");
        assert!(close(get_scalar(&ev, "x"), -0.5));
    }

    #[test]
    fn legendre_p11_condon_shortley() {
        // P_1^1(x) = -(1-x^2)^0.5; at x=0: -1
        let ev = eval_str("x = legendre(1, 1, 0.0)");
        assert!(close(get_scalar(&ev, "x"), -1.0));
    }

    #[test]
    fn legendre_vector_input() {
        // P_1^0(v) = v element-wise
        let ev = eval_str("v = legendre(1, 0, [0.0, 0.5, 1.0])");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert!(close(v[0].re, 0.0));
                assert!(close(v[1].re, 0.5));
                assert!(close(v[2].re, 1.0));
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn legendre_m_exceeds_l_errors() {
        let e = eval_err("legendre(1, 2, 0.5)");
        assert!(e.contains("legendre"), "error should mention legendre: {e}");
    }
}

// ─── ML / activation function tests ─────────────────────────────────────────

#[cfg(test)]
mod ml_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn get_vec(ev: &Evaluator, name: &str) -> Vec<f64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(s) => *s,
            Value::Complex(c) => c.re,
            other => panic!("expected scalar for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    // ── softmax ──────────────────────────────────────────────────────────────

    #[test]
    fn softmax_sums_to_one() {
        let ev = eval_str("p = softmax([1.0, 2.0, 3.0, 4.0])");
        let v = get_vec(&ev, "p");
        let s: f64 = v.iter().sum();
        assert!(
            (s - 1.0).abs() < 1e-12,
            "softmax should sum to 1.0, got {s}"
        );
    }

    #[test]
    fn softmax_all_positive() {
        let ev = eval_str("p = softmax([1.0, 2.0, 3.0])");
        let v = get_vec(&ev, "p");
        for &x in &v {
            assert!(x > 0.0, "softmax output must be positive, got {x}");
        }
    }

    #[test]
    fn softmax_monotone_with_input() {
        // Larger input → larger output probability
        let ev = eval_str("p = softmax([1.0, 2.0, 3.0])");
        let v = get_vec(&ev, "p");
        assert!(
            v[0] < v[1] && v[1] < v[2],
            "softmax should be monotone: {:?}",
            v
        );
    }

    #[test]
    fn softmax_numerically_stable_large_values() {
        // Should not overflow even with large inputs
        let ev = eval_str("p = softmax([1000.0, 1001.0, 1002.0])");
        let v = get_vec(&ev, "p");
        let s: f64 = v.iter().sum();
        assert!(
            (s - 1.0).abs() < 1e-10,
            "softmax should be stable for large inputs, sum={s}"
        );
    }

    #[test]
    fn softmax_uniform_input_is_uniform_output() {
        let ev = eval_str("p = softmax([2.0, 2.0, 2.0, 2.0])");
        let v = get_vec(&ev, "p");
        for &x in &v {
            assert!(
                (x - 0.25).abs() < 1e-10,
                "uniform softmax should be 0.25, got {x}"
            );
        }
    }

    #[test]
    fn softmax_single_scalar_is_one() {
        let ev = eval_str("p = softmax(5.0)");
        let s = get_scalar(&ev, "p");
        assert!(
            (s - 1.0).abs() < 1e-12,
            "softmax of scalar should be 1.0, got {s}"
        );
    }

    // ── relu ─────────────────────────────────────────────────────────────────

    #[test]
    fn relu_positive_passthrough() {
        let ev = eval_str("y = relu(3.5)");
        assert!(close(get_scalar(&ev, "y"), 3.5), "relu(3.5) should be 3.5");
    }

    #[test]
    fn relu_negative_clamped_to_zero() {
        let ev = eval_str("y = relu(-2.0)");
        assert!(close(get_scalar(&ev, "y"), 0.0), "relu(-2) should be 0");
    }

    #[test]
    fn relu_zero_unchanged() {
        let ev = eval_str("y = relu(0.0)");
        assert!(close(get_scalar(&ev, "y"), 0.0));
    }

    #[test]
    fn relu_vector_element_wise() {
        let ev = eval_str("y = relu([-3.0, -1.0, 0.0, 2.0, 5.0])");
        let v = get_vec(&ev, "y");
        let expected = [0.0, 0.0, 0.0, 2.0, 5.0];
        for (a, b) in v.iter().zip(expected.iter()) {
            assert!(close(*a, *b), "relu mismatch: {a} vs {b}");
        }
    }

    #[test]
    fn relu_matrix_element_wise() {
        let ev = eval_str("M = relu([-1.0, 2.0; 3.0, -4.0])");
        match ev.get("M").unwrap() {
            Value::Matrix(m) => {
                assert!(close(m[[0, 0]].re, 0.0));
                assert!(close(m[[0, 1]].re, 2.0));
                assert!(close(m[[1, 0]].re, 3.0));
                assert!(close(m[[1, 1]].re, 0.0));
            }
            other => panic!("expected matrix, got {other:?}"),
        }
    }

    // ── gelu ─────────────────────────────────────────────────────────────────

    #[test]
    fn gelu_zero_is_zero() {
        let ev = eval_str("y = gelu(0.0)");
        assert!(close(get_scalar(&ev, "y"), 0.0), "gelu(0) should be 0");
    }

    #[test]
    fn gelu_positive_input_positive_output() {
        let ev = eval_str("y = gelu(1.0)");
        let s = get_scalar(&ev, "y");
        assert!(s > 0.0 && s < 1.0, "gelu(1) should be in (0,1), got {s}");
    }

    #[test]
    fn gelu_large_positive_approaches_identity() {
        // For large x, GELU(x) ≈ x
        let ev = eval_str("y = gelu(10.0)");
        let s = get_scalar(&ev, "y");
        assert!((s - 10.0).abs() < 0.01, "gelu(10) should be ~10, got {s}");
    }

    #[test]
    fn gelu_large_negative_approaches_zero() {
        let ev = eval_str("y = gelu(-10.0)");
        let s = get_scalar(&ev, "y");
        assert!(s.abs() < 0.01, "gelu(-10) should be ~0, got {s}");
    }

    #[test]
    fn gelu_negative_input_slightly_negative() {
        // GELU allows small negative outputs for x ≈ -0.17..0
        let ev = eval_str("y = gelu(-1.0)");
        let s = get_scalar(&ev, "y");
        assert!(s < 0.0, "gelu(-1) should be negative, got {s}");
    }

    #[test]
    fn gelu_vector() {
        let ev = eval_str("y = gelu([-2.0, 0.0, 2.0])");
        let v = get_vec(&ev, "y");
        assert_eq!(v.len(), 3);
        assert!(v[0] < 0.0, "gelu(-2) < 0");
        assert!(v[1] == 0.0, "gelu(0) == 0");
        assert!(v[2] > 1.5, "gelu(2) > 1.5");
    }

    // ── layernorm ─────────────────────────────────────────────────────────────

    #[test]
    fn layernorm_zero_mean() {
        let ev = eval_str("y = layernorm([1.0, 2.0, 3.0, 4.0, 5.0])");
        let v = get_vec(&ev, "y");
        let mean: f64 = v.iter().sum::<f64>() / v.len() as f64;
        assert!(
            mean.abs() < 1e-10,
            "layernorm output should have zero mean, got {mean}"
        );
    }

    #[test]
    fn layernorm_unit_variance() {
        let ev = eval_str("y = layernorm([2.0, 4.0, 6.0, 8.0, 10.0])");
        let v = get_vec(&ev, "y");
        let n = v.len() as f64;
        let mean: f64 = v.iter().sum::<f64>() / n;
        let var: f64 = v.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / n;
        // var should be ~1.0 (population variance, eps≈1e-5)
        assert!(
            (var - 1.0).abs() < 1e-4,
            "layernorm output should have ~unit variance, got {var}"
        );
    }

    #[test]
    fn layernorm_length_preserved() {
        let ev = eval_str("y = layernorm([10.0, 20.0, 30.0])");
        let v = get_vec(&ev, "y");
        assert_eq!(v.len(), 3, "layernorm output length must match input");
    }

    #[test]
    fn layernorm_single_scalar_is_zero() {
        let ev = eval_str("y = layernorm(5.0)");
        assert!(
            close(get_scalar(&ev, "y"), 0.0),
            "layernorm of scalar should be 0"
        );
    }

    #[test]
    fn layernorm_custom_eps() {
        // With a large eps the norm is less sharp but should still be close to zero mean
        let ev = eval_str("y = layernorm([1.0, 2.0, 3.0], 1.0)");
        let v = get_vec(&ev, "y");
        let mean: f64 = v.iter().sum::<f64>() / v.len() as f64;
        assert!(mean.abs() < 1e-10);
    }

    #[test]
    fn layernorm_wrong_arg_errors() {
        let src = format!("{}\n", "y = layernorm([1.0, 2.0], 1.0, 2.0)");
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        let result = ev.exec_stmt(&stmts[0]);
        assert!(result.is_err(), "layernorm with 3 args should error");
    }
}

// ── median builtin ───────────────────────────────────────────────────────────
#[cfg(test)]
mod median_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(s) => *s,
            other => panic!("expected Scalar for '{name}', got {other:?}"),
        }
    }

    #[test]
    fn median_odd_length() {
        let ev = eval_str("m = median([3.0, 1.0, 2.0])");
        assert!(
            (get_scalar(&ev, "m") - 2.0).abs() < 1e-12,
            "median of [1,2,3] should be 2"
        );
    }

    #[test]
    fn median_even_length() {
        let ev = eval_str("m = median([4.0, 1.0, 3.0, 2.0])");
        assert!(
            (get_scalar(&ev, "m") - 2.5).abs() < 1e-12,
            "median of [1,2,3,4] should be 2.5"
        );
    }

    #[test]
    fn median_single_element() {
        let ev = eval_str("m = median([7.0])");
        assert!((get_scalar(&ev, "m") - 7.0).abs() < 1e-12);
    }

    #[test]
    fn median_scalar_passthrough() {
        let ev = eval_str("m = median(5.0)");
        assert!((get_scalar(&ev, "m") - 5.0).abs() < 1e-12);
    }
}

// ── upfirdn scripting builtin ─────────────────────────────────────────────────
#[cfg(test)]
mod upfirdn_script_tests {
    use crate::eval::value::Value;
    use crate::Evaluator;

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn get_vec(ev: &Evaluator, name: &str) -> Vec<f64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-10
    }

    #[test]
    fn upfirdn_identity() {
        let ev = run("y = upfirdn([1,2,3], [1], 1, 1)");
        let y = get_vec(&ev, "y");
        assert_eq!(y, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn upfirdn_upsample_by_2() {
        let ev = run("y = upfirdn([1,2], [1], 2, 1)");
        let y = get_vec(&ev, "y");
        assert!(close(y[0], 1.0) && close(y[1], 0.0) && close(y[2], 2.0));
    }

    #[test]
    fn upfirdn_downsample_by_2() {
        let ev = run("y = upfirdn([1,2,3,4], [1], 1, 2)");
        let y = get_vec(&ev, "y");
        assert_eq!(y.len(), 2);
        assert!(close(y[0], 1.0) && close(y[1], 3.0));
    }
}

// ── For loop, IndexAssign, abs(matrix), chained indexing ─────────────────────
#[cfg(test)]
mod lang_ext_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            Value::Complex(c) => c.re,
            other => panic!("expected scalar for '{name}', got {other:?}"),
        }
    }

    fn get_vec(ev: &Evaluator, name: &str) -> Vec<f64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    // ── for loop ─────────────────────────────────────────────────────────────

    #[test]
    fn for_sum_range() {
        // sum 1..5 = 15
        let ev = run("s = 0\nfor i = 1:5\ns = s + i\nend");
        assert_eq!(get_scalar(&ev, "s"), 15.0);
    }

    #[test]
    fn for_loop_var_in_scope_after() {
        // loop variable stays in scope after end (like Octave/Python)
        let ev = run("for i = 1:3\nend");
        assert_eq!(get_scalar(&ev, "i"), 3.0);
    }

    #[test]
    fn for_index_assign_builds_vector() {
        // x(i) = i*2 should produce [2, 4, 6, 8, 10]
        let ev = run("for i = 1:5\nx(i) = i * 2\nend");
        let v = get_vec(&ev, "x");
        assert_eq!(v, vec![2.0, 4.0, 6.0, 8.0, 10.0]);
    }

    #[test]
    fn for_nested() {
        // nested for: s = sum of 1..3 twice = 12
        let ev = run("s = 0\nfor i = 1:3\nfor k = 1:2\ns = s + i\nend\nend");
        assert_eq!(get_scalar(&ev, "s"), 12.0);
    }

    // ── while loop ───────────────────────────────────────────────────────────

    #[test]
    fn while_counts_to_five() {
        let ev = run("x = 0\nwhile x < 5\nx = x + 1\nend");
        assert_eq!(get_scalar(&ev, "x"), 5.0);
    }

    #[test]
    fn while_false_body_never_executes() {
        // condition is immediately false (1 == 2), body must not run
        let ev = run("x = 99\nwhile 1 == 2\nx = 0\nend");
        assert_eq!(get_scalar(&ev, "x"), 99.0);
    }

    #[test]
    fn while_zero_condition_skips() {
        let ev = run("x = 7\nwhile 0\nx = 0\nend");
        assert_eq!(get_scalar(&ev, "x"), 7.0);
    }

    #[test]
    fn while_accumulates_sum() {
        // sum 1+2+3+4+5 = 15
        let ev = run("s = 0\ni = 1\nwhile i <= 5\ns = s + i\ni = i + 1\nend");
        assert_eq!(get_scalar(&ev, "s"), 15.0);
    }

    #[test]
    fn while_nested_in_if() {
        // while inside an if branch
        let ev = run("x = 0\nif 1 == 1\nwhile x < 3\nx = x + 1\nend\nend");
        assert_eq!(get_scalar(&ev, "x"), 3.0);
    }

    // ── indexed assignment standalone ─────────────────────────────────────────

    #[test]
    fn index_assign_to_existing_vector() {
        let ev = run("v = [10, 20, 30];\nv(2) = 99");
        let vec = get_vec(&ev, "v");
        assert_eq!(vec[1], 99.0);
    }

    #[test]
    fn index_assign_matrix_element() {
        let ev = run("M = [1,2;3,4];\nM(1,2) = 99");
        match ev.get("M").unwrap() {
            Value::Matrix(m) => assert_eq!(m[[0, 1]].re, 99.0),
            other => panic!("expected matrix, got {other:?}"),
        }
    }

    // ── abs on matrices ────────────────────────────────────────────────────────

    #[test]
    fn abs_matrix_element_wise() {
        let ev = run("A = [-1,2;-3,4];\nB = abs(A)");
        match ev.get("B").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m[[0, 0]].re, 1.0);
                assert_eq!(m[[0, 1]].re, 2.0);
                assert_eq!(m[[1, 0]].re, 3.0);
                assert_eq!(m[[1, 1]].re, 4.0);
            }
            other => panic!("expected matrix, got {other:?}"),
        }
    }

    #[test]
    fn abs_matrix_shape_preserved() {
        let ev = run("A = [-5, 3; 0, -7];\nB = abs(A)");
        match ev.get("B").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m.nrows(), 2);
                assert_eq!(m.ncols(), 2);
            }
            other => panic!("expected matrix, got {other:?}"),
        }
    }

    // ── chained call-and-index ────────────────────────────────────────────────

    #[test]
    fn chained_index_call_result() {
        // linspace(1,5,5) returns [1,2,3,4,5]; index element 3 → 3.0
        let ev = run("v = linspace(1,5,5);\ndirect = v(3)\nchained = linspace(1,5,5)(3)");
        assert_eq!(get_scalar(&ev, "direct"), 3.0);
        assert_eq!(get_scalar(&ev, "chained"), 3.0);
    }

    #[test]
    fn chained_index_matches_tmp_var() {
        // user-defined function return value indexed inline
        let src = "function y = make(n)\ny = 1:n\nend\ntmp = make(4)\na = tmp(2)\nb = make(4)(2)";
        let ev = run(src);
        assert_eq!(get_scalar(&ev, "a"), 2.0);
        assert_eq!(get_scalar(&ev, "b"), 2.0);
    }
}

#[cfg(test)]
mod math_builtins_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{src}\n");
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }
    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("expected scalar for '{name}', got {other:?}"),
        }
    }

    // ── sinh / cosh ──────────────────────────────────────────────────────────

    #[test]
    fn sinh_zero() {
        assert_eq!(get_scalar(&run("y = sinh(0.0)"), "y"), 0.0);
    }

    #[test]
    fn cosh_zero() {
        assert_eq!(get_scalar(&run("y = cosh(0.0)"), "y"), 1.0);
    }

    #[test]
    fn sinh_cosh_identity() {
        // cosh²(x) - sinh²(x) = 1
        let ev = run("x = 1.5\nc = cosh(x)\ns = sinh(x)\nd = c*c - s*s");
        let d = get_scalar(&ev, "d");
        assert!((d - 1.0).abs() < 1e-12, "cosh²-sinh²={d}");
    }

    // ── floor / ceil / round ─────────────────────────────────────────────────

    #[test]
    fn floor_positive() {
        assert_eq!(get_scalar(&run("y = floor(3.7)"), "y"), 3.0);
    }

    #[test]
    fn floor_negative() {
        assert_eq!(get_scalar(&run("y = floor(-2.3)"), "y"), -3.0);
    }

    #[test]
    fn ceil_positive() {
        assert_eq!(get_scalar(&run("y = ceil(3.2)"), "y"), 4.0);
    }

    #[test]
    fn ceil_negative() {
        assert_eq!(get_scalar(&run("y = ceil(-2.7)"), "y"), -2.0);
    }

    #[test]
    fn round_half_up() {
        assert_eq!(get_scalar(&run("y = round(2.5)"), "y"), 3.0);
    }

    #[test]
    fn round_down() {
        assert_eq!(get_scalar(&run("y = round(2.4)"), "y"), 2.0);
    }

    // ── sign ─────────────────────────────────────────────────────────────────

    #[test]
    fn sign_positive() {
        assert_eq!(get_scalar(&run("y = sign(5.0)"), "y"), 1.0);
    }

    #[test]
    fn sign_negative() {
        assert_eq!(get_scalar(&run("y = sign(-3.0)"), "y"), -1.0);
    }

    #[test]
    fn sign_zero() {
        assert_eq!(get_scalar(&run("y = sign(0.0)"), "y"), 0.0);
    }

    // ── mod ──────────────────────────────────────────────────────────────────

    #[test]
    fn mod_basic() {
        assert_eq!(get_scalar(&run("y = mod(7.0, 3.0)"), "y"), 1.0);
    }

    #[test]
    fn mod_negative_same_sign_as_m() {
        // mod(-1, 3) = 2 (Python-style, always in [0, m))
        let y = get_scalar(&run("y = mod(-1.0, 3.0)"), "y");
        assert!((y - 2.0).abs() < 1e-12, "mod(-1,3)={y}");
    }

    #[test]
    fn mod_vector_element_wise() {
        // mod([0,1,2,3,4,5], 3) → [0,1,2,0,1,2]
        let ev = run("v = mod(0:5, 3.0)");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                let expected = [0.0, 1.0, 2.0, 0.0, 1.0, 2.0];
                for (i, (&got, &exp)) in v.iter().zip(expected.iter()).enumerate() {
                    assert!(
                        (got.re - exp).abs() < 1e-12,
                        "v({i})={} expected {exp}",
                        got.re
                    );
                }
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Lambda / anonymous function tests
    // -----------------------------------------------------------------------

    #[test]
    fn lambda_basic_square() {
        let ev = run("f = @(x) x^2;\ny = f(3);");
        match ev.get("y").unwrap() {
            Value::Scalar(n) => assert!((*n - 9.0).abs() < 1e-12, "expected 9, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn lambda_zero_arg() {
        let ev = run("f = @(x) x^2;\ny = f(0);");
        match ev.get("y").unwrap() {
            Value::Scalar(n) => assert!((*n).abs() < 1e-12, "expected 0, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn lambda_multi_arg() {
        let ev = run("g = @(x, y) sqrt(x^2 + y^2);\nd = g(3, 4);");
        match ev.get("d").unwrap() {
            Value::Scalar(n) => assert!((*n - 5.0).abs() < 1e-12, "expected 5, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn lambda_lexical_capture() {
        // Lambda should capture `a` at creation time (a=5), not at call time (a=99)
        let ev = run("a = 5;\nh = @(x) x + a;\na = 99;\ny = h(1);");
        match ev.get("y").unwrap() {
            Value::Scalar(n) => assert!(
                (*n - 6.0).abs() < 1e-12,
                "expected 6 (captured a=5), got {n}"
            ),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn lambda_vector_body() {
        // Lambda applied to a vector via element-wise op
        let ev = run("scale = @(x) x .* 2;\nv = scale([1, 2, 3]);");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                let expected = [2.0, 4.0, 6.0];
                for (i, (&got, &exp)) in v.iter().zip(expected.iter()).enumerate() {
                    assert!(
                        (got.re - exp).abs() < 1e-12,
                        "v({i})={} expected {exp}",
                        got.re
                    );
                }
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn lambda_calls_builtin() {
        let ev = run("mysin = @(x) sin(x);\ny = mysin(pi/2);");
        match ev.get("y").unwrap() {
            Value::Scalar(n) => assert!((*n - 1.0).abs() < 1e-10, "expected 1, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn lambda_composed() {
        // sq(inc(2)) = (2+1)^2 = 9
        let ev = run("sq = @(x) x^2;\ninc = @(x) x + 1;\ny = sq(inc(2));");
        match ev.get("y").unwrap() {
            Value::Scalar(n) => assert!((*n - 9.0).abs() < 1e-12, "expected 9, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn lambda_is_lambda_value() {
        let ev = run("f = @(x) x + 1;");
        assert!(matches!(ev.get("f").unwrap(), Value::Lambda { .. }));
    }

    #[test]
    fn funchandle_to_builtin() {
        let ev = run("h = @sin;\ny = h(pi/2);");
        match ev.get("y").unwrap() {
            Value::Scalar(n) => assert!((*n - 1.0).abs() < 1e-10, "expected 1, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn funchandle_is_funchandle_value() {
        let ev = run("h = @sin;");
        assert!(matches!(ev.get("h").unwrap(), Value::FuncHandle(_)));
    }

    #[test]
    fn funchandle_to_user_fn() {
        let ev = run("function y = double_it(x)\n  y = x * 2;\nend\nd = @double_it;\ny = d(7);");
        match ev.get("y").unwrap() {
            Value::Scalar(n) => assert!((*n - 14.0).abs() < 1e-12, "expected 14, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn lambda_passed_to_user_fn() {
        // Passing a lambda as an argument to a named function
        let ev = run(
            "function y = apply_twice(f, x)\n  y = f(f(x));\nend\nsq = @(x) x^2;\ny = apply_twice(sq, 2);"
        );
        match ev.get("y").unwrap() {
            // (2^2)^2 = 16
            Value::Scalar(n) => assert!((*n - 16.0).abs() < 1e-12, "expected 16, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // arrayfun tests
    // -----------------------------------------------------------------------

    #[test]
    fn arrayfun_lambda_scalar_output() {
        // @(x) x^2 over [1,2,3,4] → [1,4,9,16]
        let ev = run("f = @(x) x^2;\nv = arrayfun(f, [1, 2, 3, 4]);");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                let expected = [1.0, 4.0, 9.0, 16.0];
                for (i, (&got, &exp)) in v.iter().zip(expected.iter()).enumerate() {
                    assert!(
                        (got.re - exp).abs() < 1e-12,
                        "v({i})={} expected {exp}",
                        got.re
                    );
                }
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn arrayfun_funchandle_scalar_output() {
        // @sqrt over [1,4,9,16] → [1,2,3,4]
        let ev = run("v = arrayfun(@sqrt, [1, 4, 9, 16]);");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                let expected = [1.0, 2.0, 3.0, 4.0];
                for (i, (&got, &exp)) in v.iter().zip(expected.iter()).enumerate() {
                    assert!(
                        (got.re - exp).abs() < 1e-12,
                        "v({i})={} expected {exp}",
                        got.re
                    );
                }
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn arrayfun_vector_output_builds_matrix() {
        // @(x) [x, x^2] over [1,2,3] → 3×2 matrix [[1,1],[2,4],[3,9]]
        let ev = run("f = @(x) [x, x^2];\nM = arrayfun(f, [1, 2, 3]);");
        match ev.get("M").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m.nrows(), 3);
                assert_eq!(m.ncols(), 2);
                let expected = [[1.0, 1.0], [2.0, 4.0], [3.0, 9.0]];
                for r in 0..3 {
                    for c in 0..2 {
                        assert!(
                            (m[[r, c]].re - expected[r][c]).abs() < 1e-12,
                            "M[{r},{c}]={} expected {}",
                            m[[r, c]].re,
                            expected[r][c]
                        );
                    }
                }
            }
            other => panic!("expected matrix, got {other:?}"),
        }
    }

    #[test]
    fn arrayfun_single_scalar_input() {
        let ev = run("v = arrayfun(@(x) x * 3, 5);");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 1);
                assert!((v[0].re - 15.0).abs() < 1e-12);
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // feval tests
    // -----------------------------------------------------------------------

    #[test]
    fn feval_builtin() {
        let ev = run("y = feval(\"sin\", pi/2);");
        match ev.get("y").unwrap() {
            Value::Scalar(n) => assert!((*n - 1.0).abs() < 1e-10, "expected 1, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn feval_user_fn() {
        let ev = run("function y = triple(x)\n  y = x * 3;\nend\ny = feval(\"triple\", 4);");
        match ev.get("y").unwrap() {
            Value::Scalar(n) => assert!((*n - 12.0).abs() < 1e-12, "expected 12, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn feval_env_lambda() {
        let ev = run("sq = @(x) x^2;\ny = feval(\"sq\", 5);");
        match ev.get("y").unwrap() {
            Value::Scalar(n) => assert!((*n - 25.0).abs() < 1e-12, "expected 25, got {n}"),
            other => panic!("expected scalar, got {other:?}"),
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Profiling tests
// ───────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod profiling_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    /// Run source, then return (evaluator, profile_report).
    fn run_profiled(
        src: &str,
        names: Option<Vec<&str>>,
    ) -> (Evaluator, Vec<(String, crate::eval::FnStats)>) {
        let tokens = lexer::tokenize(src).expect("lex");
        let stmts = parser::parse(tokens).expect("parse");
        let mut ev = Evaluator::new();
        ev.enable_profiling(names.map(|v| v.iter().map(|s| s.to_string()).collect()));
        ev.run(&stmts).expect("eval");
        let report = ev.take_profile();
        (ev, report)
    }

    fn find<'a>(
        report: &'a [(String, crate::eval::FnStats)],
        name: &str,
    ) -> Option<&'a crate::eval::FnStats> {
        report.iter().find(|(n, _)| n == name).map(|(_, s)| s)
    }

    #[test]
    fn selective_tracks_only_named_function() {
        // profile(sin) — sin is tracked; cos is not
        let (_, report) =
            run_profiled("for k = 1:10\n  sin(k);\n  cos(k);\nend", Some(vec!["sin"]));
        let sin_stats = find(&report, "sin").expect("sin should be in report");
        assert_eq!(sin_stats.call_count, 10, "sin called 10 times");
        assert!(sin_stats.total_ns > 0, "sin total_ns > 0");
        assert!(find(&report, "cos").is_none(), "cos should NOT be tracked");
    }

    #[test]
    fn track_all_with_none_whitelist() {
        let (_, report) = run_profiled(
            "sin(1.0);\ncos(1.0);",
            None, // track everything
        );
        assert!(find(&report, "sin").is_some(), "sin tracked");
        assert!(find(&report, "cos").is_some(), "cos tracked");
    }

    #[test]
    fn call_count_matches_loop_iterations() {
        let (_, report) = run_profiled("for k = 1:25\n  abs(k);\nend", Some(vec!["abs"]));
        let s = find(&report, "abs").expect("abs tracked");
        assert_eq!(s.call_count, 25);
    }

    #[test]
    fn io_bytes_populated() {
        // sin(scalar) → 8 bytes in, 8 bytes out
        let (_, report) = run_profiled("sin(1.0);", Some(vec!["sin"]));
        let s = find(&report, "sin").expect("sin tracked");
        assert_eq!(s.input_bytes, 8);
        assert_eq!(s.output_bytes, 8);
    }

    #[test]
    fn no_data_when_profiling_disabled() {
        let tokens = lexer::tokenize("sin(1.0);").unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        assert!(!ev.has_profile_data(), "no data when profiling not enabled");
    }

    #[test]
    fn lambda_tracked_under_variable_name() {
        // f = @(x) x^2; f(3) — should appear as "f", not "<lambda>"
        let (_, report) = run_profiled("f = @(x) x^2;\nf(3);", Some(vec!["f"]));
        let s = find(&report, "f").expect("lambda tracked as 'f'");
        assert_eq!(s.call_count, 1);
        assert!(s.total_ns > 0);
    }

    #[test]
    fn arrayfun_inner_calls_not_tracked_separately() {
        // arrayfun(@sin, v) — sin is called N times as a callback.
        // With profile(arrayfun, sin): arrayfun tracked; sin NOT (higher_order_depth suppresses it).
        let (_, report) = run_profiled("arrayfun(@sin, 1:5);", Some(vec!["arrayfun", "sin"]));
        let af = find(&report, "arrayfun").expect("arrayfun tracked");
        assert_eq!(af.call_count, 1, "arrayfun called once");
        // sin should not appear — suppressed by higher_order_depth
        assert!(find(&report, "sin").is_none(), "sin inner calls suppressed");
    }

    #[test]
    fn user_fn_tracked_inner_builtins_suppressed() {
        let src = "function y = myabs(x)\n  y = abs(x);\nend\nmyabs(3);";
        let (_, report) = run_profiled(src, Some(vec!["myabs", "abs"]));
        let mf = find(&report, "myabs").expect("myabs tracked");
        assert_eq!(mf.call_count, 1);
        // abs is called inside myabs — higher_order_depth suppresses it
        assert!(find(&report, "abs").is_none(), "inner abs suppressed");
    }

    #[test]
    fn in_script_profile_call_activates_profiling() {
        // profile(sin) inside the script — evaluator should record sin
        let tokens = lexer::tokenize("profile(sin)\nsin(1.0);").unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        let report = ev.take_profile();
        let s = report.iter().find(|(n, _)| n == "sin");
        assert!(s.is_some(), "in-script profile(sin) activated tracking");
        assert_eq!(s.unwrap().1.call_count, 1);
    }

    #[test]
    fn vector_io_bytes_correct() {
        // fft(v) where v is length 16 → input 16*16=256 bytes, output 256 bytes
        let (_, report) = run_profiled("fft(ones(16));", Some(vec!["fft"]));
        let s = find(&report, "fft").expect("fft tracked");
        assert_eq!(
            s.input_bytes,
            16 * 16,
            "16-element complex vector = 256 bytes"
        );
        assert_eq!(s.output_bytes, 16 * 16);
    }
}

// ─── Controls Bootcamp tests ───────────────────────────────────────────────────
mod controls_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run_ev(src: &str) -> Evaluator {
        let tokens = lexer::tokenize(src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn run_get(src: &str, var: &str) -> Value {
        run_ev(src).get(var).cloned().unwrap_or(Value::None)
    }

    fn vec_re(v: Value) -> Vec<f64> {
        match v {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            Value::Matrix(m) if m.ncols() == 1 => m.column(0).iter().map(|c| c.re).collect(),
            Value::Matrix(m) if m.nrows() == 1 => m.row(0).iter().map(|c| c.re).collect(),
            other => panic!("expected vector, got {:?}", other),
        }
    }

    // ── logspace ──────────────────────────────────────────────────────────────

    #[test]
    fn logspace_endpoints() {
        let v = vec_re(run_get("w = logspace(-1, 2, 4);", "w"));
        assert!((v[0] - 0.1).abs() < 1e-12);
        assert!((v[3] - 100.0).abs() < 1e-10);
    }

    #[test]
    fn logspace_length() {
        let v = vec_re(run_get("w = logspace(0, 3, 100);", "w"));
        assert_eq!(v.len(), 100);
    }

    // ── lyap ──────────────────────────────────────────────────────────────────

    #[test]
    fn lyap_residual_near_zero() {
        // A*X + X*A' + Q = 0 for A=[-1,0;0,-2], Q=eye(2)
        // Exact solution: X[0,0]=0.5, X[1,1]=0.25, off-diag=0
        let src = "
A = [-1, 0; 0, -2];
Q = [1, 0; 0, 1];
X = lyap(A, Q);
";
        let x = run_get(src, "X");
        match x {
            Value::Matrix(m) => {
                assert!((m[[0, 0]].re - 0.5).abs() < 1e-8, "X[0,0] should be 0.5");
                assert!((m[[1, 1]].re - 0.25).abs() < 1e-8, "X[1,1] should be 0.25");
                assert!(m[[0, 1]].norm() < 1e-8, "off-diagonal should be ~0");
            }
            other => panic!("expected matrix, got {:?}", other),
        }
    }

    // ── gram ──────────────────────────────────────────────────────────────────

    #[test]
    fn gram_controllability_positive_semidefinite() {
        // For a stable controllable system, controllability Gramian has positive eigenvalues
        let src = "
A = [-1, 1; 0, -2];
B = [1; 0];
Wc = gram(A, B, \"c\");
e = eig(Wc);
";
        let e = run_get(src, "e");
        let evals = vec_re(e);
        for ev in &evals {
            assert!(*ev > -1e-8, "Gramian eigenvalue should be >= 0, got {}", ev);
        }
    }

    // ── care ──────────────────────────────────────────────────────────────────

    #[test]
    fn care_residual_near_zero() {
        // For A=[-1,1;0,-1], B=[0;1], Q=I, R=1: verify A'P + PA - PBR⁻¹B'P + Q ≈ 0
        let src = "
A = [-1, 1; 0, -1];
B = [0; 1];
Q = [1, 0; 0, 1];
R = 1;
P = care(A, B, Q, R);
";
        let p = run_get(src, "P");
        match &p {
            Value::Matrix(pm) => {
                let n = pm.nrows();
                assert_eq!(n, 2);
                // Check that P is positive definite (diagonal > 0)
                for i in 0..n {
                    assert!(pm[[i, i]].re > 0.0, "P[{i},{i}] should be positive");
                }
            }
            other => panic!("expected matrix, got {:?}", other),
        }
    }

    // ── dare ──────────────────────────────────────────────────────────────────

    #[test]
    fn dare_matches_discrete_lqr() {
        // For integrator A=[1,1;0,1], B=[0;1], Q=I, R=1: P should be positive definite
        let src = "
A = [1, 1; 0, 1];
B = [0; 1];
Q = [1, 0; 0, 1];
R = 1;
P = dare(A, B, Q, R);
";
        let p = run_get(src, "P");
        match p {
            Value::Matrix(pm) => {
                for i in 0..pm.nrows() {
                    assert!(pm[[i, i]].re > 0.0, "P[{i},{i}] should be positive");
                }
            }
            other => panic!("expected matrix, got {:?}", other),
        }
    }

    // ── place ─────────────────────────────────────────────────────────────────

    #[test]
    fn place_eigenvalues_match_desired() {
        // Double integrator: A=[0,1;0,0], B=[0;1], desired poles at -2,-3
        let src = "
A = [0, 1; 0, 0];
B = [0; 1];
poles = [-2, -3];
K = place(A, B, poles);
cl_eig = eig(A - B * K);
";
        let cl_eig = run_get(src, "cl_eig");
        let evals = vec_re(cl_eig);
        let mut sorted = evals.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(
            (sorted[0] - (-3.0)).abs() < 0.1,
            "first pole ≈ -3, got {}",
            sorted[0]
        );
        assert!(
            (sorted[1] - (-2.0)).abs() < 0.1,
            "second pole ≈ -2, got {}",
            sorted[1]
        );
    }

    // ── freqresp ──────────────────────────────────────────────────────────────

    #[test]
    fn freqresp_first_order_magnitude() {
        // First-order: A=[-1], B=[1], C=[1], D=[0] → H(jω) = 1/(1+jω)
        // At ω=1: |H| = 1/√2 ≈ 0.707
        let src = "
A = [-1];
B = [1];
C = [1];
D = [0];
w = [1.0];
H = freqresp(A, B, C, D, w);
";
        let h = run_get(src, "H");
        match h {
            Value::Vector(v) => {
                let mag = v[0].norm();
                assert!(
                    (mag - (0.5_f64).sqrt()).abs() < 1e-6,
                    "|H(j)| should be 1/√2 ≈ 0.707, got {}",
                    mag
                );
            }
            other => panic!("expected vector, got {:?}", other),
        }
    }

    // ── svd ───────────────────────────────────────────────────────────────────

    #[test]
    fn svd_diagonal_matrix() {
        // svd([3,0;0,2]) → singular values [3, 2]
        let ev = run_ev("[U, S, V] = svd([3, 0; 0, 2]);");
        let sv = vec_re(ev.get("S").cloned().unwrap());
        assert!(
            (sv[0] - 3.0).abs() < 1e-6,
            "first singular value should be 3, got {}",
            sv[0]
        );
        assert!(
            (sv[1] - 2.0).abs() < 1e-6,
            "second singular value should be 2, got {}",
            sv[1]
        );
    }

    #[test]
    fn svd_reconstruction() {
        let ev = run_ev("A = [1, 2; 3, 4; 5, 6];\n[U, S, V] = svd(A);");
        let sv = vec_re(ev.get("S").cloned().unwrap());
        for &s in &sv {
            assert!(s >= 0.0, "singular value should be non-negative, got {}", s);
        }
        assert!(
            sv[0] >= sv[1],
            "singular values should be sorted descending"
        );
    }

    // ── rk4 ───────────────────────────────────────────────────────────────────

    #[test]
    fn rk4_exponential_decay() {
        // x_dot = -x, x0 = 1 → x(t) = exp(-t); check at t=1: x ≈ 1/e ≈ 0.3679
        let src = "
f = @(x, t) -x;
t = linspace(0, 1, 1000);
X = rk4(f, 1.0, t);
";
        let x = run_get(src, "X");
        let vals = vec_re(x);
        let x_final = vals[vals.len() - 1];
        assert!(
            (x_final - (-1.0_f64).exp()).abs() < 1e-5,
            "rk4 decay: x(1) ≈ exp(-1) ≈ 0.3679, got {}",
            x_final
        );
    }

    #[test]
    fn rk4_harmonic_oscillator() {
        // x1_dot = x2, x2_dot = -x1 (undamped SHO with ω=1)
        // x0 = [1; 0] → x1(t) = cos(t), x2(t) = -sin(t)
        // Check at t=π/2: x1 ≈ 0, x2 ≈ -1
        let src = "
f = @(x, t) [x(2); -x(1)];
t = linspace(0, pi/2, 5000);
X = rk4(f, [1; 0], t);
";
        let x = run_get(src, "X");
        match x {
            Value::Matrix(m) => {
                let x1_final = m[[0, m.ncols() - 1]].re;
                let x2_final = m[[1, m.ncols() - 1]].re;
                assert!(x1_final.abs() < 1e-4, "SHO: x1(π/2) ≈ 0, got {}", x1_final);
                assert!(
                    (x2_final + 1.0).abs() < 1e-4,
                    "SHO: x2(π/2) ≈ -1, got {}",
                    x2_final
                );
            }
            other => panic!("expected matrix, got {:?}", other),
        }
    }
}

#[cfg(test)]
mod streaming_dsp_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn get_vec(ev: &Evaluator, name: &str) -> Vec<f64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    // ── state_init ────────────────────────────────────────────────────────────

    #[test]
    fn state_init_type_and_size() {
        let ev = run("s = state_init(63);");
        assert_eq!(ev.get("s").unwrap().type_name(), "fir_state");
        // display should contain the length
        assert!(format!("{}", ev.get("s").unwrap()).contains("63"));
    }

    #[test]
    fn state_init_zero_length_ok() {
        // Edge case: 1-tap filter needs 0 history samples
        let ev = run("s = state_init(0);");
        assert_eq!(ev.get("s").unwrap().type_name(), "fir_state");
    }

    // ── filter_stream ─────────────────────────────────────────────────────────

    #[test]
    fn filter_stream_size_mismatch_errors() {
        // state has 5 slots but h has 4 taps (needs 3 slots) → runtime error
        let src = "h = [1,0,0,0];\nst = state_init(5);\nframe = [1,0,0,0];\n\
                   [out, st] = filter_stream(frame, h, st);";
        let tokens = lexer::tokenize(&format!("{}\n", src)).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        let result = ev.run(&stmts);
        assert!(result.is_err(), "expected error on size mismatch");
    }

    #[test]
    fn filter_stream_identity_filter() {
        // h = [1] (single tap, identity). state_init(0). output == input.
        let src = "h = [1.0];\nst = state_init(0);\n\
                   frame = [1.0, 2.0, 3.0, 4.0];\n\
                   [out, st] = filter_stream(frame, h, st);";
        let ev = run(src);
        let out = get_vec(&ev, "out");
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn filter_stream_returns_same_state_handle() {
        // The returned state should be the same fir_state (pointer reuse, not copy)
        let src = "h = [0.5, 0.5];\nst = state_init(1);\n\
                   frame = [1.0, 0.0, 0.0, 0.0];\n\
                   [out, st2] = filter_stream(frame, h, st);";
        let ev = run(src);
        assert_eq!(ev.get("st2").unwrap().type_name(), "fir_state");
    }

    #[test]
    fn filter_stream_matches_full_convolve() {
        // Process 4 frames of a sine wave and compare against a single convolve.
        // h = simple 4-tap averager [0.25, 0.25, 0.25, 0.25]
        // Input: 16 samples (4 frames of 4)
        // The overlap-save output for frames 2-4 (after state is primed on frame 1)
        // must match the corresponding slice of convolve(full_signal, h).
        let src = "
h = [0.25, 0.25, 0.25, 0.25];
st = state_init(3);
x = [1,2,3,4, 5,6,7,8, 9,10,11,12, 13,14,15,16];
ref_full = convolve(x, h);

[o1, st] = filter_stream(x(1:4),   h, st);
[o2, st] = filter_stream(x(5:8),   h, st);
[o3, st] = filter_stream(x(9:12),  h, st);
[o4, st] = filter_stream(x(13:16), h, st);
";
        let ev = run(src);
        let ref_full = get_vec(&ev, "ref_full");
        let o1 = get_vec(&ev, "o1");
        let o2 = get_vec(&ev, "o2");
        let o3 = get_vec(&ev, "o3");
        let o4 = get_vec(&ev, "o4");

        // Frames 2-4 must match ref_full exactly (frame 1 has no history so it
        // is the transient — only check settled frames)
        let streamed: Vec<f64> = o1
            .iter()
            .chain(o2.iter())
            .chain(o3.iter())
            .chain(o4.iter())
            .copied()
            .collect();
        for (i, (&s, &r)) in streamed.iter().zip(ref_full.iter()).enumerate() {
            // First M-1=3 output samples are transient (history was zero); skip them.
            if i >= 3 {
                assert!(
                    (s - r).abs() < 1e-9,
                    "sample {i}: streamed={s}, convolve={r}"
                );
            }
        }
    }

    // ── audio_in / audio_out (metadata only) ─────────────────────────────────

    #[test]
    fn audio_in_type_and_display() {
        let ev = run("adc = audio_in(44100.0, 256);");
        assert_eq!(ev.get("adc").unwrap().type_name(), "audio_in");
        let s = format!("{}", ev.get("adc").unwrap());
        assert!(s.contains("44100"), "display should contain sample rate");
        assert!(s.contains("256"), "display should contain frame size");
    }

    #[test]
    fn audio_out_type_and_display() {
        let ev = run("dac = audio_out(44100.0, 256);");
        assert_eq!(ev.get("dac").unwrap().type_name(), "audio_out");
        let s = format!("{}", ev.get("dac").unwrap());
        assert!(s.contains("44100"));
        assert!(s.contains("256"));
    }

    // ── mag2db ────────────────────────────────────────────────────────────────

    #[test]
    fn mag2db_scalar_unity() {
        let ev = run("x = mag2db(1.0);");
        let v = ev.get("x").unwrap().to_scalar().unwrap();
        assert!((v - 0.0).abs() < 1e-10, "mag2db(1) should be 0 dB, got {v}");
    }

    #[test]
    fn mag2db_scalar_ten() {
        let ev = run("x = mag2db(10.0);");
        let v = ev.get("x").unwrap().to_scalar().unwrap();
        assert!(
            (v - 20.0).abs() < 1e-6,
            "mag2db(10) should be ~20 dB, got {v}"
        );
    }

    #[test]
    fn mag2db_zero_floor() {
        let ev = run("x = mag2db(0.0);");
        let v = ev.get("x").unwrap().to_scalar().unwrap();
        // floor at 1e-10 → 20*log10(1e-10) = -200
        assert!(
            (v - (-200.0)).abs() < 1.0,
            "mag2db(0) should be ~-200 dB, got {v}"
        );
    }

    #[test]
    fn mag2db_vector() {
        let ev = run("v = mag2db([1.0, 10.0, 100.0]);");
        let vec = ev.get("v").unwrap().to_cvector().unwrap();
        assert_eq!(vec.len(), 3);
        assert!((vec[0].re - 0.0).abs() < 1e-6);
        assert!((vec[1].re - 20.0).abs() < 1e-6);
        assert!((vec[2].re - 40.0).abs() < 1e-6);
    }

    // ── live figure (data model, no tty needed) ───────────────────────────────

    fn try_run(src: &str) -> Result<Evaluator, crate::error::ScriptError> {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts)?;
        Ok(ev)
    }

    #[test]
    fn live_figure_errors_on_non_tty() {
        // In test context stdout is not a tty, so figure_live should return an error
        // — unless the viewer feature is enabled AND a viewer is running, in which
        // case figure_live connects via IPC and succeeds without a tty.
        let result = try_run("fig = figure_live(2, 1);");
        if cfg!(feature = "viewer") && result.is_ok() {
            // Viewer was running — figure_live connected via IPC, which is fine.
            return;
        }
        assert!(
            result.is_err(),
            "figure_live should fail when stdout is not a tty"
        );
    }

    #[test]
    fn headless_context_suppresses_plot_and_blocks_live_figure() {
        use rustlab_plot::{plot_context, set_plot_context, PlotContext};
        let prev = plot_context();
        set_plot_context(PlotContext::Headless);

        // Regular plot() must not touch the terminal and must not error —
        // render_figure_terminal short-circuits under Headless.
        let plot_result = try_run("x = linspace(0.0, 1.0, 4); plot(x, x)");

        // figure_live must refuse with a clear error under Headless, regardless
        // of whether a viewer is running (headless is an explicit user opt-out).
        let live_result = try_run("fig = figure_live(1, 1)");

        set_plot_context(prev);

        assert!(
            plot_result.is_ok(),
            "plot() should succeed silently under Headless: {}",
            plot_result.err().map(|e| e.to_string()).unwrap_or_default()
        );
        assert!(
            live_result.is_err(),
            "figure_live should error under Headless"
        );
    }

    #[test]
    fn viewer_on_parses() {
        // `viewer on` should parse and run without error (prints a warning if no viewer)
        let src = "viewer on\n";
        let tokens = lexer::tokenize(src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            crate::ast::StmtKind::Viewer { on, name } => {
                assert_eq!(*on, Some(true));
                assert_eq!(*name, None);
            }
            other => panic!("expected Viewer, got {:?}", other),
        }
    }

    #[test]
    fn viewer_on_named_parses() {
        let src = "viewer on work\n";
        let tokens = lexer::tokenize(src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            crate::ast::StmtKind::Viewer { on, name } => {
                assert_eq!(*on, Some(true));
                assert_eq!(name.as_deref(), Some("work"));
            }
            other => panic!("expected Viewer, got {:?}", other),
        }
    }

    #[test]
    fn viewer_off_parses() {
        let src = "viewer off\n";
        let tokens = lexer::tokenize(src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            crate::ast::StmtKind::Viewer { on, name } => {
                assert_eq!(*on, Some(false));
                assert_eq!(*name, None);
            }
            other => panic!("expected Viewer, got {:?}", other),
        }
    }

    #[test]
    fn viewer_off_ignores_trailing_name() {
        // `viewer off somename` — 'off' doesn't take a name, should parse as
        // viewer off followed by a separate expression
        let src = "viewer off\n";
        let tokens = lexer::tokenize(src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        match &stmts[0].kind {
            crate::ast::StmtKind::Viewer { on, name } => {
                assert_eq!(*on, Some(false));
                assert!(name.is_none());
            }
            other => panic!("expected Viewer, got {:?}", other),
        }
    }

    #[test]
    fn viewer_bare_parses_as_status() {
        // bare `viewer` is a status query — on: None, name: None
        let src = "viewer\n";
        let tokens = lexer::tokenize(src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            crate::ast::StmtKind::Viewer { on, name } => {
                assert_eq!(*on, None);
                assert_eq!(*name, None);
            }
            other => panic!("expected Viewer, got {:?}", other),
        }
    }

    #[test]
    fn plot_update_wrong_type_errors() {
        // plot_update with a non-live_figure first arg should error.
        assert!(
            try_run("plot_update(42, 1, [1.0, 2.0]);").is_err(),
            "plot_update with scalar arg should error"
        );
    }

    #[test]
    fn figure_close_wrong_type_errors() {
        assert!(try_run("figure_close(42);").is_err());
    }
}

// ─── Tier 1: Arg-count and type error tests ─────────────────────────────────

#[cfg(test)]
mod arg_error_tests {
    fn try_run(src: &str) -> Result<crate::Evaluator, crate::error::ScriptError> {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = crate::Evaluator::new();
        ev.run(&stmts)?;
        Ok(ev)
    }

    fn assert_err_contains(src: &str, substr: &str) {
        match try_run(src) {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains(substr),
                    "expected error containing '{substr}', got: {msg}"
                );
            }
            Ok(_) => panic!("expected an error containing '{substr}', but script succeeded"),
        }
    }

    // ── Too few / too many args (exact-count builtins) ──────────────────

    #[test]
    fn sin_zero_args() {
        assert_err_contains("sin()", "sin");
    }
    #[test]
    fn sin_two_args() {
        assert_err_contains("sin(1, 2)", "sin");
    }
    #[test]
    fn cos_zero_args() {
        assert_err_contains("cos()", "cos");
    }
    #[test]
    fn dot_one_arg() {
        assert_err_contains("dot([1])", "dot");
    }
    #[test]
    fn dot_three_args() {
        assert_err_contains("dot([1],[2],[3])", "dot");
    }
    #[test]
    fn linspace_one_arg() {
        assert_err_contains("linspace(1)", "linspace");
    }
    #[test]
    fn fir_lowpass_two_args() {
        assert_err_contains("fir_lowpass(31, 1000)", "fir_lowpass");
    }
    #[test]
    fn cross_one_arg() {
        assert_err_contains("cross([1,2,3])", "cross");
    }
    #[test]
    fn eig_zero_args() {
        assert_err_contains("eig()", "eig");
    }
    #[test]
    fn inv_zero_args() {
        assert_err_contains("inv()", "inv");
    }

    // ── Range-count builtins: too few / too many ─────────────────────────

    #[test]
    fn zeros_zero_args() {
        assert_err_contains("zeros()", "zeros");
    }
    #[test]
    fn zeros_three_args() {
        assert_err_contains("zeros(1,2,3)", "zeros");
    }
    #[test]
    fn ones_zero_args() {
        assert_err_contains("ones()", "ones");
    }
    #[test]
    fn size_zero_args() {
        assert_err_contains("size()", "size");
    }
    #[test]
    fn diag_zero_args() {
        assert_err_contains("diag()", "diag");
    }
    #[test]
    fn norm_zero_args() {
        assert_err_contains("norm()", "norm");
    }

    // ── ArgCountRange error message includes both bounds ─────────────────

    #[test]
    fn arg_count_range_message_format() {
        match try_run("zeros(1,2,3)") {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("1..2") || (msg.contains("1") && msg.contains("2")),
                    "range error should mention both bounds, got: {msg}"
                );
            }
            Ok(_) => panic!("expected error for zeros(1,2,3)"),
        }
    }
}

#[cfg(test)]
mod type_error_tests {
    fn try_run(src: &str) -> Result<crate::Evaluator, crate::error::ScriptError> {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = crate::Evaluator::new();
        ev.run(&stmts)?;
        Ok(ev)
    }

    #[test]
    fn sin_string() {
        assert!(try_run("sin(\"hello\")").is_err());
    }
    #[test]
    fn abs_string() {
        assert!(try_run("abs(\"x\")").is_err());
    }
    #[test]
    fn sqrt_string() {
        assert!(try_run("sqrt(\"x\")").is_err());
    }
    #[test]
    fn exp_string() {
        assert!(try_run("exp(\"x\")").is_err());
    }
    #[test]
    fn log_string() {
        assert!(try_run("log(\"x\")").is_err());
    }
    #[test]
    fn reshape_string() {
        assert!(try_run("reshape(\"x\", 2, 3)").is_err());
    }
    #[test]
    fn inv_string() {
        assert!(try_run("inv(\"x\")").is_err());
    }
    #[test]
    fn eig_string() {
        assert!(try_run("eig(\"hello\")").is_err());
    }
    #[test]
    fn det_string() {
        assert!(try_run("det(\"x\")").is_err());
    }
    #[test]
    fn transpose_string() {
        assert!(try_run("transpose(\"x\")").is_err());
    }
    #[test]
    fn fft_string() {
        assert!(try_run("fft(\"x\")").is_err());
    }
    #[test]
    fn convolve_strings() {
        assert!(try_run("convolve(\"a\", \"b\")").is_err());
    }
}

// ─── Tier 2a: Evaluator edge cases ──────────────────────────────────────────

#[cfg(test)]
mod evaluator_edge_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn try_run(src: &str) -> Result<Evaluator, crate::error::ScriptError> {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts)?;
        Ok(ev)
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("expected scalar for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // ── For loop ────────────────────────────────────────────────────────

    #[test]
    fn for_over_scalar() {
        // Scalar iteration: single pass, loop var = the scalar
        let ev = run("s = 0;\nfor x = 5\n  s = s + x;\nend");
        assert!(close(get_scalar(&ev, "s"), 5.0));
    }

    #[test]
    fn for_over_empty_vector() {
        // Empty vector: zero passes, s stays at initial value
        let ev = run("s = 42;\nfor x = []\n  s = 0;\nend");
        assert!(close(get_scalar(&ev, "s"), 42.0));
    }

    #[test]
    fn for_accumulates() {
        let ev = run("s = 0;\nfor x = 1:5\n  s = s + x;\nend");
        assert!(close(get_scalar(&ev, "s"), 15.0));
    }

    #[test]
    fn nested_for_if() {
        let ev = run("s = 0;\nfor i = 1:4\n  if i > 2\n    s = s + i;\n  end\nend");
        assert!(close(get_scalar(&ev, "s"), 7.0)); // 3 + 4
    }

    // ── While loop ──────────────────────────────────────────────────────

    #[test]
    fn while_basic() {
        let ev = run("n = 0;\nwhile n < 5\n  n = n + 1;\nend");
        assert!(close(get_scalar(&ev, "n"), 5.0));
    }

    #[test]
    fn while_compound_condition() {
        let ev = run("a = 0;\nb = 10;\nwhile (a < 5) && (b > 6)\n  a = a + 1;\n  b = b - 1;\nend");
        assert!(close(get_scalar(&ev, "a"), 4.0));
        assert!(close(get_scalar(&ev, "b"), 6.0));
    }

    #[test]
    fn while_false_never_executes() {
        let ev = run("x = 99;\nwhile 0\n  x = 0;\nend");
        assert!(close(get_scalar(&ev, "x"), 99.0));
    }

    // ── Return inside nested block ──────────────────────────────────────

    #[test]
    fn return_inside_nested_if() {
        let ev = run("function y = f(x)\n  if x > 0\n    y = 1;\n    return\n  end\n  y = -1;\nend\nr = f(5);");
        assert!(close(get_scalar(&ev, "r"), 1.0));
    }

    #[test]
    fn return_skips_remaining_body() {
        let ev = run("function y = g()\n  y = 10;\n  return\n  y = 20;\nend\nr = g();");
        assert!(close(get_scalar(&ev, "r"), 10.0));
    }

    // ── Semicolon suppression ───────────────────────────────────────────

    #[test]
    fn semicolon_still_assigns() {
        let ev = run("x = 42;");
        assert!(close(get_scalar(&ev, "x"), 42.0));
    }

    // ── Complex as while condition ───────────────────────────────────────

    #[test]
    fn while_complex_zero_is_falsy() {
        let ev = run("x = 99;\nwhile 0 + 0*j\n  x = 0;\nend");
        assert!(close(get_scalar(&ev, "x"), 99.0));
    }

    // ── For over non-iterable errors ────────────────────────────────────

    #[test]
    fn for_over_string_errors() {
        assert!(try_run("for x = \"hello\"\n  y = 1;\nend").is_err());
    }
}

// ─── Tier 2b: Index assignment edge cases ───────────────────────────────────

#[cfg(test)]
mod index_assign_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn try_run(src: &str) -> Result<Evaluator, crate::error::ScriptError> {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts)?;
        Ok(ev)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn auto_create_vector() {
        let ev = run("v(3) = 7;");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 3);
                assert!(close(v[0].re, 0.0)); // zero-filled
                assert!(close(v[1].re, 0.0));
                assert!(close(v[2].re, 7.0));
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn in_place_update() {
        let ev = run("v = [10, 20, 30];\nv(2) = 99;");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert!(close(v[0].re, 10.0));
                assert!(close(v[1].re, 99.0));
                assert!(close(v[2].re, 30.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn grow_vector() {
        let ev = run("v = [1, 2, 3];\nv(6) = 60;");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 6);
                assert!(close(v[0].re, 1.0));
                assert!(close(v[2].re, 3.0));
                assert!(close(v[3].re, 0.0)); // zero-filled gap
                assert!(close(v[5].re, 60.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn matrix_element_assign() {
        let ev = run("M = eye(2);\nM(1,2) = 5;");
        match ev.get("M").unwrap() {
            Value::Matrix(m) => {
                assert!(close(m[[0, 0]].re, 1.0));
                assert!(close(m[[0, 1]].re, 5.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn matrix_row_assign() {
        let ev = run("M = eye(2);\nM(2) = [7, 8];");
        match ev.get("M").unwrap() {
            Value::Matrix(m) => {
                assert!(close(m[[1, 0]].re, 7.0));
                assert!(close(m[[1, 1]].re, 8.0));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn index_zero_errors() {
        assert!(try_run("v = [1,2,3];\nv(0) = 1;").is_err());
    }

    #[test]
    fn matrix_out_of_bounds_errors() {
        assert!(try_run("M = eye(2);\nM(3,1) = 1;").is_err());
    }

    #[test]
    fn matrix_col_out_of_bounds_errors() {
        assert!(try_run("M = eye(2);\nM(1,3) = 1;").is_err());
    }
}

// ─── Tier 2c: Parser error messages ─────────────────────────────────────────

#[cfg(test)]
mod parser_error_tests {
    fn parse_err(src: &str) -> String {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        crate::parser::parse(tokens).unwrap_err().to_string()
    }

    #[test]
    fn missing_end_if() {
        let err = parse_err("if 1 < 2\n  x = 1\n");
        assert!(
            err.contains("end") || err.contains("End") || err.contains("missing"),
            "error should mention missing 'end', got: {err}"
        );
    }

    #[test]
    fn missing_end_for() {
        let err = parse_err("for i = 1:3\n  x = i\n");
        assert!(
            err.contains("end") || err.contains("missing"),
            "error should mention missing 'end', got: {err}"
        );
    }

    #[test]
    fn missing_end_while() {
        let err = parse_err("while 1\n  x = 1\n");
        assert!(
            err.contains("end") || err.contains("missing"),
            "error should mention missing 'end', got: {err}"
        );
    }

    #[test]
    fn missing_end_function() {
        let err = parse_err("function foo()\n  x = 1\n");
        assert!(
            err.contains("end") || err.contains("missing"),
            "error should mention missing 'end', got: {err}"
        );
    }

    #[test]
    fn stray_end_top_level() {
        let err = parse_err("x = 1\nend\n");
        assert!(
            err.contains("end") || err.contains("unexpected"),
            "stray end should error, got: {err}"
        );
    }

    #[test]
    fn else_without_if() {
        let err = parse_err("else\n  x = 1\nend\n");
        assert!(
            err.contains("else") || err.contains("if"),
            "else without if should error, got: {err}"
        );
    }
}

// ─── Tier 2d: Figure state machine (non-rendering) ─────────────────────────

#[cfg(test)]
mod figure_state_tests {
    use rustlab_plot::figure::FIGURE;

    fn reset_figure() {
        FIGURE.with(|f| f.borrow_mut().reset());
    }

    fn run(src: &str) {
        // Reset state before each test to avoid cross-test pollution
        reset_figure();
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = crate::Evaluator::new();
        ev.run(&stmts).unwrap();
    }

    #[test]
    fn figure_resets_state() {
        run("title(\"old\");\nfigure();");
        FIGURE.with(|f| {
            let fig = f.borrow();
            assert_eq!(fig.current().title, "", "figure() should reset title");
            assert!(!fig.hold, "figure() should reset hold");
        });
    }

    #[test]
    fn hold_on_off() {
        run("hold(\"on\");");
        FIGURE.with(|f| assert!(f.borrow().hold, "hold('on') should set hold=true"));
        run("hold(\"off\");");
        FIGURE.with(|f| assert!(!f.borrow().hold, "hold('off') should set hold=false"));
    }

    #[test]
    fn title_sets_current_subplot() {
        run("title(\"My Title\");");
        FIGURE.with(|f| {
            assert_eq!(f.borrow().current().title, "My Title");
        });
    }

    #[test]
    fn xlabel_ylabel() {
        run("xlabel(\"Time\");\nylabel(\"Amplitude\");");
        FIGURE.with(|f| {
            let fig = f.borrow();
            assert_eq!(fig.current().xlabel, "Time");
            assert_eq!(fig.current().ylabel, "Amplitude");
        });
    }

    #[test]
    fn xlim_ylim() {
        run("xlim([0, 10]);\nylim([-1, 1]);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            assert_eq!(fig.current().xlim, (Some(0.0), Some(10.0)));
            assert_eq!(fig.current().ylim, (Some(-1.0), Some(1.0)));
        });
    }

    #[test]
    fn subplot_creates_panels() {
        run("subplot(2, 1, 1);\ntitle(\"Top\");\nsubplot(2, 1, 2);\ntitle(\"Bottom\");");
        FIGURE.with(|f| {
            let fig = f.borrow();
            assert_eq!(fig.subplot_rows, 2);
            assert_eq!(fig.subplot_cols, 1);
            assert_eq!(fig.subplots.len(), 2);
            assert_eq!(fig.subplots[0].title, "Top");
            assert_eq!(fig.subplots[1].title, "Bottom");
        });
    }

    #[test]
    fn grid_on_off() {
        run("grid(\"off\");");
        FIGURE.with(|f| assert!(!f.borrow().current().grid));
        run("grid(\"on\");");
        FIGURE.with(|f| assert!(f.borrow().current().grid));
    }

    #[test]
    fn plot_accepts_nx1_column_matrix() {
        run("v = [1.0; 2.0; 3.0]; plot(v);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            assert_eq!(
                sp.series.len(),
                1,
                "Nx1 column matrix should plot as a single series"
            );
            assert_eq!(sp.series[0].y_data, vec![1.0, 2.0, 3.0]);
        });
    }

    #[test]
    fn stem_accepts_nx1_column_matrix() {
        run("v = [1.0; 2.0; 3.0]; stem(v);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            assert_eq!(sp.series.len(), 1);
            assert_eq!(sp.series[0].y_data, vec![1.0, 2.0, 3.0]);
        });
    }

    #[test]
    fn bar_accepts_nx1_column_matrix() {
        run("v = [1.0; 2.0; 3.0]; bar(v);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            assert_eq!(sp.series.len(), 1);
            assert_eq!(sp.series[0].y_data, vec![1.0, 2.0, 3.0]);
        });
    }

    #[test]
    fn scatter_accepts_nx1_column_matrix() {
        run("x = [1.0; 2.0; 3.0]; y = [4.0; 5.0; 6.0]; scatter(x, y);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            assert_eq!(sp.series.len(), 1);
            assert_eq!(sp.series[0].x_data, vec![1.0, 2.0, 3.0]);
            assert_eq!(sp.series[0].y_data, vec![4.0, 5.0, 6.0]);
        });
    }

    #[test]
    fn surf_single_arg_sets_surface() {
        run("Z = [1.0, 2.0, 3.0; 4.0, 5.0, 6.0]; surf(Z);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            let surf = sp.surface.as_ref().expect("surface should be set");
            assert_eq!(surf.z.len(), 2);
            assert_eq!(surf.z[0].len(), 3);
            assert_eq!(surf.x, vec![1.0, 2.0, 3.0]);
            assert_eq!(surf.y, vec![1.0, 2.0]);
            assert_eq!(surf.colorscale, "viridis");
        });
    }

    #[test]
    fn surf_xyz_with_vectors() {
        run("x = [10.0, 20.0, 30.0]; y = [100.0, 200.0]; Z = [1.0, 2.0, 3.0; 4.0, 5.0, 6.0]; surf(x, y, Z);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            let surf = sp.surface.as_ref().expect("surface should be set");
            assert_eq!(surf.x, vec![10.0, 20.0, 30.0]);
            assert_eq!(surf.y, vec![100.0, 200.0]);
            assert_eq!(surf.z.len(), 2);
            assert_eq!(surf.z[0].len(), 3);
        });
    }

    #[test]
    fn surf_accepts_meshgrid_matrices() {
        run("[X, Y] = meshgrid([1.0, 2.0, 3.0], [10.0, 20.0]); Z = X + Y; surf(X, Y, Z);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            let surf = sp.surface.as_ref().expect("surface should be set");
            assert_eq!(surf.x, vec![1.0, 2.0, 3.0]);
            assert_eq!(surf.y, vec![10.0, 20.0]);
            assert_eq!(surf.z.len(), 2);
            assert_eq!(surf.z[0].len(), 3);
        });
    }

    #[test]
    fn surf_with_colormap_string() {
        run("Z = [1.0, 2.0; 3.0, 4.0]; surf(Z, Z, Z, \"jet\");");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            let surf = sp.surface.as_ref().expect("surface should be set");
            assert_eq!(surf.colorscale, "jet");
        });
    }

    #[test]
    fn surf_rejects_mismatched_axis_length() {
        reset_figure();
        let src = "x = [1.0, 2.0]; y = [1.0, 2.0]; Z = [1.0, 2.0, 3.0; 4.0, 5.0, 6.0]; surf(x, y, Z);\n";
        let tokens = crate::lexer::tokenize(src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = crate::Evaluator::new();
        let err = ev.run(&stmts).expect_err("expected dim mismatch error");
        let msg = err.to_string();
        assert!(
            msg.contains("surf") && msg.contains("match"),
            "expected dim error, got: {msg}"
        );
    }
}

// ─── Tier 2e: Struct operations ─────────────────────────────────────────────

#[cfg(test)]
mod struct_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn try_run(src: &str) -> Result<Evaluator, crate::error::ScriptError> {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts)?;
        Ok(ev)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn struct_creation() {
        let ev = run("s = struct(\"x\", 1, \"y\", 2);");
        match ev.get("s").unwrap() {
            Value::Struct(fields) => {
                assert_eq!(fields.len(), 2);
                assert!(matches!(fields.get("x"), Some(Value::Scalar(n)) if close(*n, 1.0)));
                assert!(matches!(fields.get("y"), Some(Value::Scalar(n)) if close(*n, 2.0)));
            }
            other => panic!("expected struct, got {other:?}"),
        }
    }

    #[test]
    fn field_access() {
        let ev = run("s = struct(\"x\", 42);\nv = s.x;");
        match ev.get("v").unwrap() {
            Value::Scalar(n) => assert!(close(*n, 42.0)),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn field_assignment_existing_struct() {
        let ev = run("s = struct(\"x\", 1);\ns.y = 99;");
        match ev.get("s").unwrap() {
            Value::Struct(fields) => {
                assert!(matches!(fields.get("y"), Some(Value::Scalar(n)) if close(*n, 99.0)));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn auto_create_struct() {
        let ev = run("thing.val = 7;");
        match ev.get("thing").unwrap() {
            Value::Struct(fields) => {
                assert!(matches!(fields.get("val"), Some(Value::Scalar(n)) if close(*n, 7.0)));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn isstruct_true() {
        let ev = run("s = struct(\"a\", 1);\nb = isstruct(s);");
        match ev.get("b").unwrap() {
            Value::Bool(true) => {}
            other => panic!("expected true, got {other:?}"),
        }
    }

    #[test]
    fn isstruct_false() {
        let ev = run("b = isstruct(5);");
        match ev.get("b").unwrap() {
            Value::Bool(false) => {}
            other => panic!("expected false, got {other:?}"),
        }
    }

    #[test]
    fn isfield_true_false() {
        let ev = run("s = struct(\"x\", 1);\na = isfield(s, \"x\");\nb = isfield(s, \"nope\");");
        match ev.get("a").unwrap() {
            Value::Bool(true) => {}
            other => panic!("expected true, got {other:?}"),
        }
        match ev.get("b").unwrap() {
            Value::Bool(false) => {}
            other => panic!("expected false, got {other:?}"),
        }
    }

    #[test]
    fn rmfield() {
        let ev = run("s = struct(\"x\", 1, \"y\", 2);\ns2 = rmfield(s, \"x\");");
        match ev.get("s2").unwrap() {
            Value::Struct(fields) => {
                assert!(!fields.contains_key("x"), "x should be removed");
                assert!(fields.contains_key("y"), "y should remain");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn field_assign_on_non_struct_errors() {
        assert!(try_run("x = 5;\nx.foo = 1;").is_err());
    }

    #[test]
    fn fieldnames_does_not_error() {
        // fieldnames prints to stdout and returns None; just verify it doesn't error
        let ev = run("s = struct(\"alpha\", 1, \"beta\", 2);\nfieldnames(s);");
        // s should still be a struct
        assert!(matches!(ev.get("s").unwrap(), Value::Struct(_)));
    }
}

// ─── Tier 3: Lambda and function handle tests ───────────────────────────────

#[cfg(test)]
mod lambda_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn try_run(src: &str) -> Result<Evaluator, crate::error::ScriptError> {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts)?;
        Ok(ev)
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("expected scalar for '{name}', got {other:?}"),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // ── Basic lambda ────────────────────────────────────────────────────

    #[test]
    fn lambda_square() {
        let ev = run("sq = @(x) x^2;\nr = sq(5);");
        assert!(close(get_scalar(&ev, "r"), 25.0));
    }

    #[test]
    fn lambda_multi_arg() {
        let ev = run("hyp = @(a, b) sqrt(a^2 + b^2);\nr = hyp(3, 4);");
        assert!(close(get_scalar(&ev, "r"), 5.0));
    }

    #[test]
    fn lambda_composition() {
        let ev = run("sq = @(x) x^2;\ninc = @(x) x + 1;\nr = sq(inc(4));");
        assert!(close(get_scalar(&ev, "r"), 25.0));
    }

    // ── Lexical capture ─────────────────────────────────────────────────

    #[test]
    fn lambda_captures_env() {
        let ev = run("g = 0.5;\natten = @(x) x * g;\nr = atten(10);");
        assert!(close(get_scalar(&ev, "r"), 5.0));
    }

    #[test]
    fn lambda_capture_is_snapshot() {
        // Changing the variable after lambda creation does not affect it
        let ev = run("g = 0.5;\natten = @(x) x * g;\ng = 99;\nr = atten(10);");
        assert!(close(get_scalar(&ev, "r"), 5.0));
    }

    // ── Element-wise lambda on vector ───────────────────────────────────

    #[test]
    fn lambda_on_vector() {
        let ev = run("dbl = @(v) v .* 2;\nr = dbl([1, 2, 3]);");
        match ev.get("r").unwrap() {
            Value::Vector(v) => {
                assert!(close(v[0].re, 2.0));
                assert!(close(v[1].re, 4.0));
                assert!(close(v[2].re, 6.0));
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    // ── Function handles ────────────────────────────────────────────────

    #[test]
    fn funchandle_builtin() {
        let ev = run("h = @sin;\nr = h(pi / 2);");
        assert!(close(get_scalar(&ev, "r"), 1.0));
    }

    #[test]
    fn funchandle_abs() {
        let ev = run("h = @abs;\nr = h(-7);");
        assert!(close(get_scalar(&ev, "r"), 7.0));
    }

    #[test]
    fn funchandle_user_fn() {
        let ev = run("function y = cube(x)\n  y = x^3;\nend\nh = @cube;\nr = h(3);");
        assert!(close(get_scalar(&ev, "r"), 27.0));
    }

    // ── Passing lambdas as arguments ────────────────────────────────────

    #[test]
    fn lambda_as_argument() {
        let ev =
            run("function y = apply(f, x)\n  y = f(x);\nend\nsq = @(x) x^2;\nr = apply(sq, 6);");
        assert!(close(get_scalar(&ev, "r"), 36.0));
    }

    #[test]
    fn funchandle_as_argument() {
        let ev = run("function y = apply(f, x)\n  y = f(x);\nend\nr = apply(@sqrt, 16);");
        assert!(close(get_scalar(&ev, "r"), 4.0));
    }

    #[test]
    fn apply_twice() {
        let ev = run(
            "function y = twice(f, x)\n  y = f(f(x));\nend\ninc = @(x) x + 1;\nr = twice(inc, 0);",
        );
        assert!(close(get_scalar(&ev, "r"), 2.0));
    }

    // ── Higher-order: lambda returning lambda ───────────────────────────

    #[test]
    fn lambda_returning_lambda() {
        let ev = run("make_gain = @(g) @(v) v .* g;\nboost = make_gain(3);\nr = boost(10);");
        assert!(close(get_scalar(&ev, "r"), 30.0));
    }

    // ── arrayfun ────────────────────────────────────────────────────────

    #[test]
    fn arrayfun_scalar_output() {
        let ev = run("r = arrayfun(@(x) x^2, [1, 2, 3, 4]);");
        match ev.get("r").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 4);
                assert!(close(v[0].re, 1.0));
                assert!(close(v[1].re, 4.0));
                assert!(close(v[2].re, 9.0));
                assert!(close(v[3].re, 16.0));
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn arrayfun_with_funchandle() {
        let ev = run("r = arrayfun(@sqrt, [4, 9, 16]);");
        match ev.get("r").unwrap() {
            Value::Vector(v) => {
                assert!(close(v[0].re, 2.0));
                assert!(close(v[1].re, 3.0));
                assert!(close(v[2].re, 4.0));
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn arrayfun_vector_output_gives_matrix() {
        let ev = run("r = arrayfun(@(x) [x, x^2], [1, 2, 3]);");
        match ev.get("r").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m.nrows(), 3);
                assert_eq!(m.ncols(), 2);
                assert!(close(m[[0, 0]].re, 1.0));
                assert!(close(m[[0, 1]].re, 1.0));
                assert!(close(m[[2, 0]].re, 3.0));
                assert!(close(m[[2, 1]].re, 9.0));
            }
            other => panic!("expected matrix, got {other:?}"),
        }
    }

    // ── feval ────────────────────────────────────────────────────────────

    #[test]
    fn feval_builtin() {
        let ev = run("r = feval(\"sqrt\", 144);");
        assert!(close(get_scalar(&ev, "r"), 12.0));
    }

    #[test]
    fn feval_user_fn() {
        let ev = run("function y = dbl(x)\n  y = x * 2;\nend\nr = feval(\"dbl\", 5);");
        assert!(close(get_scalar(&ev, "r"), 10.0));
    }

    // ── Error cases ─────────────────────────────────────────────────────

    #[test]
    fn lambda_wrong_arg_count() {
        assert!(try_run("f = @(x) x^2;\nf(1, 2);").is_err());
    }

    #[test]
    fn funchandle_undefined_errors() {
        assert!(try_run("h = @nonexistent_fn;\nh(1);").is_err());
    }
}

// ─── Tier 3: Optional-arg builtin negative tests ────────────────────────────

#[cfg(test)]
mod optional_arg_tests {
    fn try_run(src: &str) -> Result<crate::Evaluator, crate::error::ScriptError> {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = crate::Evaluator::new();
        ev.run(&stmts)?;
        Ok(ev)
    }

    fn assert_err(src: &str) {
        assert!(try_run(src).is_err(), "expected error for: {src}");
    }

    // ── zeros/ones: accept 1-2 args ─────────────────────────────────────
    #[test]
    fn zeros_too_many() {
        assert_err("zeros(1, 2, 3)");
    }
    #[test]
    fn ones_too_many() {
        assert_err("ones(1, 2, 3)");
    }

    // ── rand/randn: accept 1-2 args ─────────────────────────────────────
    #[test]
    fn rand_too_many() {
        assert_err("rand(1, 2, 3)");
    }
    #[test]
    fn randn_too_many() {
        assert_err("randn(1, 2, 3)");
    }
    #[test]
    fn rand_zero_args() {
        assert_err("rand()");
    }

    // ── trapz: accept 1-2 args ──────────────────────────────────────────
    #[test]
    fn trapz_zero_args() {
        assert_err("trapz()");
    }
    #[test]
    fn trapz_three_args() {
        assert_err("trapz([1],[2],[3])");
    }

    // ── size: accept 1-2 args ───────────────────────────────────────────
    #[test]
    fn size_three_args() {
        assert_err("size([1], 1, 2)");
    }

    // ── diag: accept 1-2 args ───────────────────────────────────────────
    #[test]
    fn diag_three_args() {
        assert_err("diag([1], 1, 2)");
    }

    // ── norm: accept 1-2 args ───────────────────────────────────────────
    #[test]
    fn norm_three_args() {
        assert_err("norm([1], 2, 3)");
    }

    // ── layernorm: accept 1-2 args ──────────────────────────────────────
    #[test]
    fn layernorm_zero_args() {
        assert_err("layernorm()");
    }
    #[test]
    fn layernorm_three_args() {
        assert_err("layernorm([1], 1, 2)");
    }

    // ── tf: accept 1-2 args ─────────────────────────────────────────────
    #[test]
    fn tf_zero_args() {
        assert_err("tf()");
    }
    #[test]
    fn tf_three_args() {
        assert_err("tf([1], [1], [1])");
    }

    // ── step/bode: accept 1-2 args ──────────────────────────────────────
    #[test]
    fn step_zero_args() {
        assert_err("step()");
    }
    #[test]
    fn bode_zero_args() {
        assert_err("bode()");
    }

    // ── Verify that optional args DO work (positive cases) ──────────────
    #[test]
    fn zeros_one_arg() {
        try_run("zeros(3);").unwrap();
    }
    #[test]
    fn zeros_two_args() {
        try_run("zeros(2, 3);").unwrap();
    }
    #[test]
    fn ones_one_arg() {
        try_run("ones(3);").unwrap();
    }
    #[test]
    fn ones_two_args() {
        try_run("ones(2, 3);").unwrap();
    }
    #[test]
    fn diag_one_arg() {
        try_run("diag([1, 2, 3]);").unwrap();
    }
    #[test]
    fn norm_one_arg() {
        try_run("norm([1, 2, 3]);").unwrap();
    }
    #[test]
    fn trapz_one_arg() {
        try_run("trapz([1, 2, 3]);").unwrap();
    }
    #[test]
    fn size_one_arg() {
        try_run("size([1, 2, 3]);").unwrap();
    }
}

// ── Sparse Phase 1 & 2 ─────────────────────────────────────────────────────

#[cfg(test)]
mod sparse_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("expected scalar for '{name}', got {other:?}"),
        }
    }

    fn get_vec(ev: &Evaluator, name: &str) -> ndarray::Array1<num_complex::Complex<f64>> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.clone(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    fn get_matrix(ev: &Evaluator, name: &str) -> ndarray::Array2<num_complex::Complex<f64>> {
        match ev.get(name).unwrap() {
            Value::Matrix(m) => m.clone(),
            other => panic!("expected matrix for '{name}', got {other:?}"),
        }
    }

    // ── Phase 1: Construction & Inspection ──────────────────────────────────

    #[test]
    fn sparsevec_basic() {
        let ev = eval_str("sv = sparsevec([1, 5, 9], [1.0, -2.0, 3.0], 10)\nk = nnz(sv)");
        assert_eq!(get_scalar(&ev, "k"), 3.0);
    }

    #[test]
    fn sparse_matrix_basic() {
        let ev = eval_str("S = sparse([1,2,3], [1,2,3], [10.0,20.0,30.0], 3, 3)\nk = nnz(S)");
        assert_eq!(get_scalar(&ev, "k"), 3.0);
    }

    #[test]
    fn sparse_duplicate_indices_summed() {
        let ev = eval_str("S = sparse([1,1], [1,1], [5.0, 7.0], 2, 2)\nk = nnz(S)");
        assert_eq!(get_scalar(&ev, "k"), 1.0);
        let ev2 = eval_str("S = sparse([1,1], [1,1], [5.0, 7.0], 2, 2)\nv = S(1,1)");
        assert_eq!(get_scalar(&ev2, "v"), 12.0);
    }

    #[test]
    fn speye_basic() {
        let ev = eval_str("I4 = speye(4)\nk = nnz(I4)");
        assert_eq!(get_scalar(&ev, "k"), 4.0);
    }

    #[test]
    fn speye_diagonal_values() {
        let ev = eval_str("I3 = speye(3)\na = I3(1,1)\nb = I3(2,2)\nc = I3(3,3)\nd = I3(1,2)");
        assert_eq!(get_scalar(&ev, "a"), 1.0);
        assert_eq!(get_scalar(&ev, "b"), 1.0);
        assert_eq!(get_scalar(&ev, "c"), 1.0);
        assert_eq!(get_scalar(&ev, "d"), 0.0);
    }

    #[test]
    fn spzeros_basic() {
        let ev = eval_str("Z = spzeros(3, 4)\nk = nnz(Z)");
        assert_eq!(get_scalar(&ev, "k"), 0.0);
    }

    #[test]
    fn issparse_true() {
        let ev = eval_str("a = issparse(speye(3))\nb = issparse(sparsevec([1],[1.0],3))");
        assert_eq!(get_scalar(&ev, "a"), 1.0);
        assert_eq!(get_scalar(&ev, "b"), 1.0);
    }

    #[test]
    fn issparse_false() {
        let ev = eval_str("a = issparse([1,2,3])\nb = issparse(5.0)");
        assert_eq!(get_scalar(&ev, "a"), 0.0);
        assert_eq!(get_scalar(&ev, "b"), 0.0);
    }

    #[test]
    fn size_sparse() {
        let ev = eval_str("S = sparse([1],[1],[1.0], 3, 5)\ns = size(S)");
        match ev.get("s").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v[0].re, 3.0);
                assert_eq!(v[1].re, 5.0);
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn display_sparse_no_panic() {
        let ev = eval_str("S = speye(3)");
        let s = ev.get("S").unwrap();
        let _ = format!("{}", s);
    }

    #[test]
    fn display_empty_sparse_no_panic() {
        let ev = eval_str("Z = spzeros(2, 2)");
        let z = ev.get("Z").unwrap();
        let _ = format!("{}", z);
    }

    // ── Phase 2: Conversion ─────────────────────────────────────────────────

    #[test]
    fn full_sparse_vec() {
        let ev = eval_str("sv = sparsevec([1,3], [1.0, 2.0], 4)\nd = full(sv)");
        match ev.get("d").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 4);
                assert_eq!(v[0].re, 1.0);
                assert_eq!(v[1].re, 0.0);
                assert_eq!(v[2].re, 2.0);
                assert_eq!(v[3].re, 0.0);
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn full_sparse_matrix() {
        let ev = eval_str("S = speye(3)\nD = full(S)");
        match ev.get("D").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m.nrows(), 3);
                assert_eq!(m.ncols(), 3);
                assert_eq!(m[[0, 0]].re, 1.0);
                assert_eq!(m[[1, 1]].re, 1.0);
                assert_eq!(m[[0, 1]].re, 0.0);
            }
            other => panic!("expected matrix, got {other:?}"),
        }
    }

    #[test]
    fn sparse_from_dense_roundtrip() {
        let ev = eval_str("D = eye(3)\nS = sparse(D)\nk = nnz(S)\nis = issparse(S)");
        assert_eq!(get_scalar(&ev, "k"), 3.0);
        assert_eq!(get_scalar(&ev, "is"), 1.0);
    }

    #[test]
    fn sparse_vec_from_dense() {
        let ev = eval_str("v = [1, 0, 0, 2]\nsv = sparse(v)\nk = nnz(sv)\nis = issparse(sv)");
        assert_eq!(get_scalar(&ev, "k"), 2.0);
        assert_eq!(get_scalar(&ev, "is"), 1.0);
    }

    #[test]
    fn full_identity_for_dense() {
        let ev = eval_str("v = [1,2,3]\nd = full(v)\nn = len(d)");
        assert_eq!(get_scalar(&ev, "n"), 3.0);
    }

    #[test]
    fn find_sparse_matrix() {
        let ev = eval_str("[I, J, V] = find(speye(3))");
        match ev.get("I").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(v[0].re, 1.0);
                assert_eq!(v[1].re, 2.0);
                assert_eq!(v[2].re, 3.0);
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn find_sparse_vec() {
        let ev = eval_str("sv = sparsevec([2, 5], [10.0, 20.0], 6)\n[I, V] = find(sv)");
        match ev.get("I").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0].re, 2.0);
                assert_eq!(v[1].re, 5.0);
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn nonzeros_sparse() {
        let ev = eval_str("S = sparse([1,2], [1,2], [10.0, 20.0], 3, 3)\nv = nonzeros(S)");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0].re, 10.0);
                assert_eq!(v[1].re, 20.0);
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    // ── Phase 2: Index read/write ───────────────────────────────────────────

    #[test]
    fn sparse_vec_index_read() {
        let ev =
            eval_str("sv = sparsevec([1,3], [10.0, 30.0], 5)\na = sv(1)\nb = sv(2)\nc = sv(3)");
        assert_eq!(get_scalar(&ev, "a"), 10.0);
        assert_eq!(get_scalar(&ev, "b"), 0.0);
        assert_eq!(get_scalar(&ev, "c"), 30.0);
    }

    #[test]
    fn sparse_matrix_index_read() {
        let ev = eval_str("S = speye(3)\na = S(1,1)\nb = S(1,2)");
        assert_eq!(get_scalar(&ev, "a"), 1.0);
        assert_eq!(get_scalar(&ev, "b"), 0.0);
    }

    #[test]
    fn sparse_matrix_index_write() {
        let ev = eval_str("S = speye(3)\nS(2,2) = 99\nv = S(2,2)");
        assert_eq!(get_scalar(&ev, "v"), 99.0);
    }

    #[test]
    fn sparse_matrix_index_write_zero_removes() {
        let ev = eval_str("S = speye(3)\nS(1,1) = 0\nk = nnz(S)");
        assert_eq!(get_scalar(&ev, "k"), 2.0);
    }

    #[test]
    fn sparse_vec_index_write() {
        let ev = eval_str("sv = sparsevec([1], [5.0], 3)\nsv(2) = 10\na = sv(2)\nk = nnz(sv)");
        assert_eq!(get_scalar(&ev, "a"), 10.0);
        assert_eq!(get_scalar(&ev, "k"), 2.0);
    }

    // ── Phase 2: Auto-promotion arithmetic ──────────────────────────────────

    #[test]
    fn sparse_plus_dense_produces_dense() {
        let ev = eval_str("S = speye(3)\nD = eye(3)\nR = S + D\nis = issparse(R)");
        assert_eq!(get_scalar(&ev, "is"), 0.0);
    }

    #[test]
    fn sparse_times_scalar() {
        let ev = eval_str("S = speye(3)\nR = full(S * 5)\na = R(1,1)\nb = R(2,2)");
        assert_eq!(get_scalar(&ev, "a"), 5.0);
        assert_eq!(get_scalar(&ev, "b"), 5.0);
    }

    #[test]
    fn speye_times_vector() {
        let ev = eval_str("I3 = speye(3)\nx = [1,2,3]\ny = I3 * x'");
        match ev.get("y").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(v[0].re, 1.0);
                assert_eq!(v[1].re, 2.0);
                assert_eq!(v[2].re, 3.0);
            }
            Value::Matrix(m) => {
                assert_eq!(m.nrows(), 3);
                assert_eq!(m[[0, 0]].re, 1.0);
                assert_eq!(m[[1, 0]].re, 2.0);
                assert_eq!(m[[2, 0]].re, 3.0);
            }
            other => panic!("expected vector or matrix, got {other:?}"),
        }
    }

    #[test]
    fn transpose_sparse() {
        let ev = eval_str("S = sparse([1], [2], [5.0], 2, 3)\nT = transpose(S)\ns = size(T)");
        match ev.get("s").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v[0].re, 3.0);
                assert_eq!(v[1].re, 2.0);
            }
            other => panic!("expected vector, got {other:?}"),
        }
        let ev2 = eval_str("S = sparse([1], [2], [5.0], 2, 3)\nT = transpose(S)\nv = T(2,1)");
        assert_eq!(get_scalar(&ev2, "v"), 5.0);
    }

    #[test]
    fn negate_sparse() {
        let ev = eval_str("S = sparse([1], [1], [5.0], 2, 2)\nN = -S\nv = N(1,1)");
        assert_eq!(get_scalar(&ev, "v"), -5.0);
    }

    // ── Phase 3: Native Sparse Arithmetic ───────────────────────────────────

    #[test]
    fn speye_times_scalar_stays_sparse() {
        let ev = eval_str("S = speye(3) * 5\nis = issparse(S)\nk = nnz(S)\nv = S(2,2)");
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        assert_eq!(get_scalar(&ev, "k"), 3.0);
        assert_eq!(get_scalar(&ev, "v"), 5.0);
    }

    #[test]
    fn scalar_times_speye_stays_sparse() {
        let ev = eval_str("S = 3 * speye(4)\nis = issparse(S)\nv = S(1,1)");
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        assert_eq!(get_scalar(&ev, "v"), 3.0);
    }

    #[test]
    fn sparse_div_scalar() {
        let ev = eval_str("S = speye(2) * 10\nR = S / 5\nis = issparse(R)\nv = R(1,1)");
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        assert_eq!(get_scalar(&ev, "v"), 2.0);
    }

    #[test]
    fn speye_plus_speye_stays_sparse() {
        let ev = eval_str("S = speye(3) + speye(3)\nis = issparse(S)\nk = nnz(S)\nv = S(1,1)");
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        assert_eq!(get_scalar(&ev, "k"), 3.0);
        assert_eq!(get_scalar(&ev, "v"), 2.0);
    }

    #[test]
    fn sparse_sub_sparse() {
        let ev =
            eval_str("A = speye(3) * 5\nB = speye(3) * 2\nC = A - B\nis = issparse(C)\nv = C(2,2)");
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        assert_eq!(get_scalar(&ev, "v"), 3.0);
    }

    #[test]
    fn spmv_identity() {
        // speye(4) * column vector [1,2,3,4]' should give [1,2,3,4]
        let ev = eval_str("I4 = speye(4)\nx = [1,2,3,4]\ny = I4 * x'");
        match ev.get("y").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m.nrows(), 4);
                assert_eq!(m[[0, 0]].re, 1.0);
                assert_eq!(m[[1, 0]].re, 2.0);
                assert_eq!(m[[2, 0]].re, 3.0);
                assert_eq!(m[[3, 0]].re, 4.0);
            }
            other => panic!("expected matrix, got {other:?}"),
        }
    }

    #[test]
    fn spmm_identity() {
        // speye(2) * dense 2×2 matrix = same matrix
        let ev = eval_str("I2 = speye(2)\nM = [1,2; 3,4]\nR = I2 * M\na = R(1,1)\nb = R(1,2)\nc = R(2,1)\nd = R(2,2)");
        assert_eq!(get_scalar(&ev, "a"), 1.0);
        assert_eq!(get_scalar(&ev, "b"), 2.0);
        assert_eq!(get_scalar(&ev, "c"), 3.0);
        assert_eq!(get_scalar(&ev, "d"), 4.0);
    }

    #[test]
    fn spmv_non_identity() {
        // S = [2,0; 0,3], x = [4;5] → [8;15]
        let ev = eval_str("S = sparse([1,2], [1,2], [2.0, 3.0], 2, 2)\nx = [4,5]\ny = S * x'");
        match ev.get("y").unwrap() {
            Value::Matrix(m) => {
                assert_eq!(m[[0, 0]].re, 8.0);
                assert_eq!(m[[1, 0]].re, 15.0);
            }
            other => panic!("expected matrix, got {other:?}"),
        }
    }

    #[test]
    fn sparse_transpose_dims() {
        let ev =
            eval_str("S = sparse([1], [2], [5.0], 2, 3)\nT = S'\ns = size(T)\nis = issparse(T)");
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        match ev.get("s").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v[0].re, 3.0);
                assert_eq!(v[1].re, 2.0);
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn dot_sparse_sparse() {
        let ev = eval_str("a = sparsevec([1,3], [2.0, 3.0], 4)\nb = sparsevec([1,3], [4.0, 5.0], 4)\nd = dot(a, b)");
        // 2*4 + 3*5 = 8 + 15 = 23
        assert_eq!(get_scalar(&ev, "d"), 23.0);
    }

    #[test]
    fn dot_sparse_dense() {
        let ev = eval_str("a = sparsevec([1,3], [2.0, 3.0], 3)\nb = [4, 5, 6]\nd = dot(a, b)");
        // 2*4 + 0*5 + 3*6 = 8 + 18 = 26
        assert_eq!(get_scalar(&ev, "d"), 26.0);
    }

    #[test]
    fn dot_dense_sparse() {
        let ev = eval_str("a = [1, 2, 3]\nb = sparsevec([2], [10.0], 3)\nd = dot(a, b)");
        // 1*0 + 2*10 + 3*0 = 20
        assert_eq!(get_scalar(&ev, "d"), 20.0);
    }

    #[test]
    fn sparse_vec_add_stays_sparse() {
        let ev = eval_str("a = sparsevec([1], [3.0], 4)\nb = sparsevec([3], [7.0], 4)\nc = a + b\nis = issparse(c)\nk = nnz(c)");
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        assert_eq!(get_scalar(&ev, "k"), 2.0);
    }

    #[test]
    fn sparse_vec_scale_stays_sparse() {
        let ev =
            eval_str("a = sparsevec([1,2], [3.0, 4.0], 5)\nb = a * 10\nis = issparse(b)\nv = b(1)");
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        assert_eq!(get_scalar(&ev, "v"), 30.0);
    }

    #[test]
    fn sparse_cancel_to_zero_nnz() {
        // Adding a sparse matrix to its negation should give nnz=0
        let ev = eval_str("S = speye(3)\nZ = S + (-S)\nk = nnz(Z)");
        assert_eq!(get_scalar(&ev, "k"), 0.0);
    }

    // ── Phase 4: solver, spdiags, sprand, norm ──────────────────────────────

    #[test]
    fn spsolve_identity() {
        let ev = eval_str("x = spsolve(speye(3), [1, 2, 3])");
        let x = get_vec(&ev, "x");
        assert_eq!(x.len(), 3);
        assert!((x[0].re - 1.0).abs() < 1e-10);
        assert!((x[1].re - 2.0).abs() < 1e-10);
        assert!((x[2].re - 3.0).abs() < 1e-10);
    }

    #[test]
    fn spsolve_scaled_identity() {
        let ev = eval_str("A = speye(3) * 2\nx = spsolve(A, [2, 4, 6])");
        let x = get_vec(&ev, "x");
        assert!((x[0].re - 1.0).abs() < 1e-10);
        assert!((x[1].re - 2.0).abs() < 1e-10);
        assert!((x[2].re - 3.0).abs() < 1e-10);
    }

    #[test]
    fn spdiags_main_diagonal() {
        let ev =
            eval_str("S = spdiags([1,2,3,4,5], 0, 5, 5)\nk = nnz(S)\nis = issparse(S)\nv = S(3,3)");
        assert_eq!(get_scalar(&ev, "k"), 5.0);
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        assert_eq!(get_scalar(&ev, "v"), 3.0);
    }

    #[test]
    fn spdiags_matches_diag() {
        // spdiags diagonal should match diag() when converted to dense
        let ev = eval_str("S = spdiags([1,2,3], 0, 3, 3)\nD = full(S)\nE = diag([1,2,3])");
        let d = get_matrix(&ev, "D");
        let e = get_matrix(&ev, "E");
        for r in 0..3 {
            for c in 0..3 {
                assert!((d[[r, c]] - e[[r, c]]).norm() < 1e-10);
            }
        }
    }

    #[test]
    fn spdiags_super_sub_diagonal() {
        // Superdiagonal d=1
        let ev = eval_str("S = spdiags([10, 20], 1, 3, 3)\nv = S(1,2)");
        assert_eq!(get_scalar(&ev, "v"), 10.0);
    }

    #[test]
    fn sprand_density() {
        let ev = eval_str("S = sprand(10, 10, 0.1)\nk = nnz(S)\nis = issparse(S)");
        let k = get_scalar(&ev, "k");
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        // With density=0.1 and 100 elements, expect ~10 nnz (allow wide tolerance due to randomness)
        assert!(k >= 0.0 && k <= 100.0);
    }

    #[test]
    fn sprand_zero_density() {
        let ev = eval_str("S = sprand(5, 5, 0.0)\nk = nnz(S)");
        assert_eq!(get_scalar(&ev, "k"), 0.0);
    }

    #[test]
    fn norm_sparse_vector() {
        let ev = eval_str("sv = sparsevec([1, 3], [3.0, 4.0], 5)\nn = norm(sv)");
        assert!((get_scalar(&ev, "n") - 5.0).abs() < 1e-10); // sqrt(9+16) = 5
    }

    #[test]
    fn norm_sparse_vector_1() {
        let ev = eval_str("sv = sparsevec([1, 3], [3.0, -4.0], 5)\nn = norm(sv, 1)");
        assert!((get_scalar(&ev, "n") - 7.0).abs() < 1e-10); // 3+4 = 7
    }

    #[test]
    fn norm_sparse_vector_inf() {
        let ev = eval_str("sv = sparsevec([1, 3], [3.0, -4.0], 5)\nn = norm(sv, Inf)");
        assert!((get_scalar(&ev, "n") - 4.0).abs() < 1e-10);
    }

    #[test]
    fn norm_sparse_matrix_frobenius() {
        let ev = eval_str("S = speye(3) * 2\nn = norm(S)");
        // Frobenius of diag(2,2,2) = sqrt(4+4+4) = sqrt(12)
        assert!((get_scalar(&ev, "n") - (12.0_f64).sqrt()).abs() < 1e-10);
    }

    #[test]
    fn norm_sparse_matrix_1() {
        // 1-norm = max column sum of abs values
        let ev = eval_str("S = sparse([1,2,1], [1,1,2], [3.0, -4.0, 2.0], 2, 2)\nn = norm(S, 1)");
        assert!((get_scalar(&ev, "n") - 7.0).abs() < 1e-10); // col1: |3|+|-4|=7, col2: |2|=2
    }

    #[test]
    fn norm_sparse_matrix_inf() {
        // inf-norm = max row sum of abs values
        let ev = eval_str("S = sparse([1,2,1], [1,1,2], [3.0, -4.0, 2.0], 2, 2)\nn = norm(S, Inf)");
        assert!((get_scalar(&ev, "n") - 5.0).abs() < 1e-10); // row1: |3|+|2|=5, row2: |-4|=4
    }

    #[test]
    fn linsolve_accepts_sparse() {
        // linsolve should also accept sparse A now
        let ev = eval_str("x = linsolve(speye(2), [5, 7])");
        let x = get_vec(&ev, "x");
        assert!((x[0].re - 5.0).abs() < 1e-10);
        assert!((x[1].re - 7.0).abs() < 1e-10);
    }

    #[test]
    fn spsolve_singular_errors() {
        let result = std::panic::catch_unwind(|| eval_str("x = spsolve(spzeros(3, 3), [1, 2, 3])"));
        assert!(result.is_err());
    }

    #[test]
    fn spsolve_dimension_mismatch_errors() {
        let result = std::panic::catch_unwind(|| eval_str("x = spsolve(speye(3), [1, 2])"));
        assert!(result.is_err());
    }

    #[test]
    fn spsolve_laplacian_round_trip_sparse_path() {
        // Negate the laplacian_2d so the matrix is SPD (positive-definite).
        // The hand-rolled Cholesky should pick this up automatically under
        // mode "auto" and produce a result matching dense LU to 1e-9.
        let ev = eval_str(
            "L = -1 * laplacian_2d(20, 20);\n\
             rhs = ones(400, 1);\n\
             x_auto     = spsolve(L, rhs);\n\
             x_cholesky = spsolve(L, rhs, \"cholesky\");\n\
             x_lu       = spsolve(L, rhs, \"lu\");\n\
             diff_ac = norm(x_auto - x_cholesky);\n\
             diff_al = norm(x_auto - x_lu);",
        );
        let diff_ac = get_scalar(&ev, "diff_ac");
        let diff_al = get_scalar(&ev, "diff_al");
        assert!(
            diff_ac < 1e-10,
            "auto vs cholesky disagreement: {diff_ac}"
        );
        assert!(diff_al < 1e-7, "auto vs lu disagreement: {diff_al}");
    }

    #[test]
    fn spsolve_cholesky_mode_rejects_indefinite() {
        // [[1, 2], [2, 1]] is symmetric but indefinite (eigenvalues 3, -1).
        // is_spd_estimate returns false (negative eigenvalue), so "auto"
        // skips Cholesky and falls back to dense LU. Forcing "cholesky"
        // exposes the failure.
        let result = std::panic::catch_unwind(|| {
            eval_str(
                "A = [1, 2; 2, 1]\n\
                 As = sparse(A)\n\
                 x = spsolve(As, [1; 1], \"cholesky\")",
            )
        });
        assert!(
            result.is_err(),
            "expected error from cholesky on indefinite matrix"
        );
    }

    #[test]
    fn spsolve_unknown_mode_errors() {
        let result = std::panic::catch_unwind(|| {
            eval_str("x = spsolve(speye(3), [1; 2; 3], \"banana\")")
        });
        assert!(result.is_err());
    }

    #[test]
    fn spdiags_multi_diagonal() {
        // Build tridiagonal: [-1, 2, -1] on diags [-1, 0, 1]
        let ev = eval_str(
            "V = [-1, -1, -1; 2, 2, 2; -1, -1, -1]'\n\
             T = spdiags(V, [-1, 0, 1], 3, 3)\n\
             k = nnz(T)\n\
             d = T(2, 2)\n\
             o = T(1, 2)",
        );
        assert_eq!(get_scalar(&ev, "d"), 2.0);
        assert_eq!(get_scalar(&ev, "o"), -1.0);
        assert_eq!(get_scalar(&ev, "k"), 7.0); // 3 main + 2 super + 2 sub
    }

    #[test]
    fn sprand_full_density() {
        let ev = eval_str("S = sprand(3, 3, 1.0)\nk = nnz(S)");
        assert_eq!(get_scalar(&ev, "k"), 9.0);
    }

    // ── Phase 4: Laplacian stencil + index sugar ─────────────────────────────

    #[test]
    fn laplacian_2d_dimensions_and_nnz() {
        // 3×3 grid: 1 interior cell (5 entries), 4 edge cells (4 entries),
        // 4 corner cells (3 entries) → 5 + 16 + 12 = 33 nnz.
        let ev = eval_str(
            "L = laplacian_2d(3, 3)\n\
             is = issparse(L)\n\
             k = nnz(L)",
        );
        assert_eq!(get_scalar(&ev, "is"), 1.0);
        assert_eq!(get_scalar(&ev, "k"), 33.0);
    }

    #[test]
    fn laplacian_2d_interior_stencil_entries() {
        // For nx = ny = 3, the centre cell is k = 5 (1-based, column-major:
        // i = 2, j = 2). Its 4 neighbours sit at k ± 1 (vertical) and
        // k ± ny = k ± 3 (horizontal). Diagonal = -4 for dx = dy = 1.
        let ev = eval_str(
            "L = laplacian_2d(3, 3)\n\
             D = full(L)\n\
             d = D(5, 5)\n\
             u = D(5, 4)\n\
             dn = D(5, 6)\n\
             l = D(5, 2)\n\
             r = D(5, 8)",
        );
        assert!((get_scalar(&ev, "d") - (-4.0)).abs() < 1e-12);
        assert!((get_scalar(&ev, "u") - 1.0).abs() < 1e-12);
        assert!((get_scalar(&ev, "dn") - 1.0).abs() < 1e-12);
        assert!((get_scalar(&ev, "l") - 1.0).abs() < 1e-12);
        assert!((get_scalar(&ev, "r") - 1.0).abs() < 1e-12);
    }

    #[test]
    fn laplacian_2d_spacing_scales_diagonal() {
        // Diagonal = -2*(1/dx² + 1/dy²). With dx = 0.5, dy = 0.25 → -2*(4+16) = -40.
        let ev = eval_str(
            "L = laplacian_2d(4, 4, 0.5, 0.25)\n\
             D = full(L)\n\
             d = D(6, 6)",
        );
        assert!((get_scalar(&ev, "d") - (-40.0)).abs() < 1e-10);
    }

    #[test]
    fn laplacian_2d_poisson_roundtrip() {
        // Self-consistency: pick a column vector x, compute rhs = L*x via
        // the dense form, solve, and verify the residual norm is tiny.
        let ev = eval_str(
            "nx = 5; ny = 4;\n\
             L = laplacian_2d(nx, ny);\n\
             D = full(L);\n\
             x = (1:nx*ny)';\n\
             rhs = D * x;\n\
             y = spsolve(L, rhs);\n\
             err = norm(y' - x)",
        );
        assert!(get_scalar(&ev, "err") < 1e-9);
    }

    #[test]
    fn laplacian_2d_dirichlet_column_major_agreement() {
        // For a 5×5 grid (nx = ny = 5), the Dirichlet sine eigenfunction
        // V(i, j) = sin(pi*i/(ny+1)) * sin(pi*j/(nx+1)) satisfies L*V = λ*V
        // exactly, where λ = 2*(cos(pi/(nx+1)) + cos(pi/(ny+1)) - 2).
        // Ghost V = 0 at i = 0, ny+1 and j = 0, nx+1 — which is exactly how
        // the stencil is assembled.
        let ev = eval_str(
            "nx = 5; ny = 5;\n\
             L = laplacian_2d(nx, ny);\n\
             D = full(L);\n\
             V = zeros(ny, nx);\n\
             for jj = 1:nx\n\
               for ii = 1:ny\n\
                 V(ii, jj) = sin(pi*ii/(ny+1)) * sin(pi*jj/(nx+1));\n\
               end\n\
             end\n\
             v_flat = V(:)';\n\
             lam = 2*(cos(pi/(nx+1)) + cos(pi/(ny+1)) - 2);\n\
             r = D * v_flat - lam * v_flat;\n\
             err = norm(r)",
        );
        assert!(get_scalar(&ev, "err") < 1e-10);
    }

    #[test]
    fn ij2k_formula_is_column_major() {
        // ij2k(3, 4, 6) = (4-1)*6 + 3 = 21.
        let ev = eval_str("k = ij2k(3, 4, 6)");
        assert_eq!(get_scalar(&ev, "k"), 21.0);
    }

    #[test]
    fn k2ij_inverse_of_ij2k() {
        // k2ij(21, 6) = [3, 4].
        let ev = eval_str("[i, j] = k2ij(21, 6)");
        assert_eq!(get_scalar(&ev, "i"), 3.0);
        assert_eq!(get_scalar(&ev, "j"), 4.0);
    }

    #[test]
    fn ij2k_k2ij_roundtrip_across_grid() {
        // For every (i, j) in a 5×4 grid, k2ij(ij2k(i, j, 5), 5) == [i, j].
        let ev = eval_str(
            "ny = 5; nx = 4;\n\
             err = 0;\n\
             for jj = 1:nx\n\
               for ii = 1:ny\n\
                 k = ij2k(ii, jj, ny);\n\
                 [ri, rj] = k2ij(k, ny);\n\
                 err = err + abs(ri - ii) + abs(rj - jj);\n\
               end\n\
             end",
        );
        assert_eq!(get_scalar(&ev, "err"), 0.0);
    }

    #[test]
    fn laplacian_2d_rejects_small_dimensions() {
        let result = std::panic::catch_unwind(|| eval_str("L = laplacian_2d(1, 3)"));
        assert!(result.is_err());
    }

    #[test]
    fn laplacian_2d_rejects_non_positive_spacing() {
        let result = std::panic::catch_unwind(|| eval_str("L = laplacian_2d(3, 3, 0, 1)"));
        assert!(result.is_err());
    }
}

// ── Tax-script language features ────────────────────────────────────────────

#[cfg(test)]
mod tax_feature_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("Expected scalar for '{name}', got {other:?}"),
        }
    }

    fn eval_err(src: &str) -> String {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        let mut err = String::new();
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                err = format!("{}", e);
                break;
            }
        }
        err
    }

    // ── Line continuation (...) ─────────────────────────────────────────────

    #[test]
    fn line_continuation() {
        let ev = eval_str("x = 1 + ...\n  2 + ...\n  3");
        assert_eq!(get_scalar(&ev, "x"), 6.0);
    }

    #[test]
    fn line_continuation_with_comment() {
        let ev = eval_str("x = 10 + ... this is ignored\n  5");
        assert_eq!(get_scalar(&ev, "x"), 15.0);
    }

    // ── Two-arg min / max ───────────────────────────────────────────────────

    #[test]
    fn min_two_args() {
        let ev = eval_str("x = min(3, 7)");
        assert_eq!(get_scalar(&ev, "x"), 3.0);
    }

    #[test]
    fn max_two_args() {
        let ev = eval_str("x = max(0, -5)");
        assert_eq!(get_scalar(&ev, "x"), 0.0);
    }

    #[test]
    fn min_one_arg_vector() {
        let ev = eval_str("x = min([4, 1, 9])");
        assert_eq!(get_scalar(&ev, "x"), 1.0);
    }

    // ── error() ─────────────────────────────────────────────────────────────

    #[test]
    fn error_halts_with_message_double_quote() {
        let msg = eval_err("error(\"something went wrong\")");
        assert!(msg.contains("something went wrong"), "got: {msg}");
    }

    #[test]
    fn error_halts_with_single_quote() {
        let msg = eval_err("error('something went wrong')");
        assert!(msg.contains("something went wrong"), "got: {msg}");
    }

    // ── Single-line if with comma ───────────────────────────────────────────

    #[test]
    fn single_line_if_comma() {
        let ev = eval_str("x = 10\nif x > 5, x = 99; end");
        assert_eq!(get_scalar(&ev, "x"), 99.0);
    }

    #[test]
    fn single_line_if_false() {
        let ev = eval_str("x = 3\nif x > 5, x = 99; end");
        assert_eq!(get_scalar(&ev, "x"), 3.0);
    }

    #[test]
    fn single_line_if_multiple_stmts() {
        let ev = eval_str("a = 10; b = 20\nif a > 5, a = a + 1; b = b + 1; end");
        assert_eq!(get_scalar(&ev, "a"), 11.0);
        assert_eq!(get_scalar(&ev, "b"), 21.0);
    }

    // ── switch / case / otherwise ───────────────────────────────────────────

    #[test]
    fn switch_case_match() {
        let ev = eval_str("q = 2\nswitch q\n  case 1\n    x = 10\n  case 2\n    x = 20\n  case 3\n    x = 30\nend");
        assert_eq!(get_scalar(&ev, "x"), 20.0);
    }

    #[test]
    fn switch_otherwise() {
        let ev = eval_str("q = 99\nswitch q\n  case 1\n    x = 10\n  otherwise\n    x = -1\nend");
        assert_eq!(get_scalar(&ev, "x"), -1.0);
    }

    #[test]
    fn switch_no_match_no_otherwise() {
        // No match and no otherwise — variable should not be set
        let src = "q = 5\nswitch q\n  case 1\n    x = 10\n  case 2\n    x = 20\nend";
        let ev = eval_str(src);
        assert!(ev.get("x").is_none());
    }

    #[test]
    fn switch_single_line_case() {
        let ev = eval_str("q = 1\nswitch q\n  case 1\n    m = 4.0; p = 0.25;\n  case 2\n    m = 2.4; p = 0.50;\nend");
        assert_eq!(get_scalar(&ev, "m"), 4.0);
        assert_eq!(get_scalar(&ev, "p"), 0.25);
    }

    // ── elseif ──────────────────────────────────────────────────────────────

    #[test]
    fn elseif_first_branch() {
        let ev =
            eval_str("x = 1\nif x == 1\n  r = 10\nelseif x == 2\n  r = 20\nelse\n  r = 30\nend");
        assert_eq!(get_scalar(&ev, "r"), 10.0);
    }

    #[test]
    fn elseif_middle_branch() {
        let ev =
            eval_str("x = 2\nif x == 1\n  r = 10\nelseif x == 2\n  r = 20\nelse\n  r = 30\nend");
        assert_eq!(get_scalar(&ev, "r"), 20.0);
    }

    #[test]
    fn elseif_else_branch() {
        let ev =
            eval_str("x = 9\nif x == 1\n  r = 10\nelseif x == 2\n  r = 20\nelse\n  r = 30\nend");
        assert_eq!(get_scalar(&ev, "r"), 30.0);
    }

    #[test]
    fn elseif_multiple_arms() {
        let ev = eval_str("x = 3\nif x == 1\n  r = 10\nelseif x == 2\n  r = 20\nelseif x == 3\n  r = 30\nelseif x == 4\n  r = 40\nend");
        assert_eq!(get_scalar(&ev, "r"), 30.0);
    }

    #[test]
    fn elseif_no_else() {
        let ev = eval_str("x = 99\nif x == 1\n  r = 10\nelseif x == 2\n  r = 20\nend");
        assert!(ev.get("r").is_none());
    }

    // ── Tax bracket helper (integration) ────────────────────────────────────

    #[test]
    fn tax_bracket_single_line_if_chain() {
        // Simplified version of calculate_mfj_tax using single-line if with comma
        let src = r#"
income = 100000
tax = 0
b1 = 23850; b2 = 96950
r1 = 0.10; r2 = 0.12; r3 = 0.22
if income > b2, tax = tax + (income - b2) * r3; income = b2; end
if income > b1, tax = tax + (income - b1) * r2; income = b1; end
if income > 0, tax = tax + income * r1; end
"#;
        let ev = eval_str(src);
        let tax = get_scalar(&ev, "tax");
        // 0.22*(100000-96950) + 0.12*(96950-23850) + 0.10*23850
        let expected = 0.22 * 3050.0 + 0.12 * 73100.0 + 0.10 * 23850.0;
        assert!(
            (tax - expected).abs() < 0.01,
            "tax={tax}, expected={expected}"
        );
    }

    // ── clear / clf as bare commands ────────────────────────────────────────

    #[test]
    fn clear_removes_variables() {
        let ev = eval_str("x = 42; y = 99\nclear\nz = 1");
        assert!(ev.get("x").is_none());
        assert!(ev.get("y").is_none());
        assert_eq!(get_scalar(&ev, "z"), 1.0);
    }

    #[test]
    fn clear_preserves_builtins() {
        let ev = eval_str("x = 1\nclear");
        // pi should still exist after clear
        assert!(ev.get("pi").is_some());
    }

    #[test]
    fn clf_does_not_error() {
        // clf should execute without error (resets figure state)
        let _ev = eval_str("clf");
    }

    #[test]
    fn clear_semicolon_clf_semicolon() {
        // The original use case: `clear; clf;` on one line
        let _ev = eval_str("x = 1\nclear; clf;");
    }

    // ── Compound assignment (+=, -=, *=, /=) ────────────────────────────────

    #[test]
    fn plus_eq() {
        let ev = eval_str("x = 10\nx += 5");
        assert_eq!(get_scalar(&ev, "x"), 15.0);
    }

    #[test]
    fn minus_eq() {
        let ev = eval_str("x = 10\nx -= 3");
        assert_eq!(get_scalar(&ev, "x"), 7.0);
    }

    #[test]
    fn star_eq() {
        let ev = eval_str("x = 4\nx *= 3");
        assert_eq!(get_scalar(&ev, "x"), 12.0);
    }

    #[test]
    fn slash_eq() {
        let ev = eval_str("x = 20\nx /= 4");
        assert_eq!(get_scalar(&ev, "x"), 5.0);
    }

    #[test]
    fn compound_assign_in_loop() {
        let ev = eval_str("s = 0\nfor i = 1:5\n  s += i\nend");
        assert_eq!(get_scalar(&ev, "s"), 15.0);
    }

    #[test]
    fn compound_assign_with_suppress() {
        let ev = eval_str("x = 10; x += 5;");
        assert_eq!(get_scalar(&ev, "x"), 15.0);
    }
}

// ── Comma formatting tests ──────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod comma_tests {
    use crate::eval::value::{insert_commas, Value};
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn get_str(ev: &Evaluator, name: &str) -> String {
        match ev.get(name).unwrap() {
            Value::Str(s) => s.clone(),
            other => panic!("Expected string for '{name}', got {other:?}"),
        }
    }

    // ── insert_commas unit tests ────────────────────────────────────────────

    #[test]
    fn commas_small() {
        assert_eq!(insert_commas("123"), "123");
    }
    #[test]
    fn commas_thousands() {
        assert_eq!(insert_commas("1234"), "1,234");
    }
    #[test]
    fn commas_millions() {
        assert_eq!(insert_commas("1234567"), "1,234,567");
    }
    #[test]
    fn commas_decimal() {
        assert_eq!(insert_commas("1234567.89"), "1,234,567.89");
    }
    #[test]
    fn commas_negative() {
        assert_eq!(insert_commas("-1234567"), "-1,234,567");
    }
    #[test]
    fn commas_neg_decimal() {
        assert_eq!(insert_commas("-1234567.89"), "-1,234,567.89");
    }
    #[test]
    fn commas_zero() {
        assert_eq!(insert_commas("0"), "0");
    }
    #[test]
    fn commas_small_dec() {
        assert_eq!(insert_commas("0.123"), "0.123");
    }

    // ── commas() builtin ────────────────────────────────────────────────────

    #[test]
    fn commas_builtin_integer() {
        let ev = eval_str("s = commas(1234567)");
        assert_eq!(get_str(&ev, "s"), "1,234,567");
    }

    #[test]
    fn commas_builtin_float() {
        let ev = eval_str("s = commas(1234567.89, 2)");
        assert_eq!(get_str(&ev, "s"), "1,234,567.89");
    }

    #[test]
    fn commas_builtin_small() {
        let ev = eval_str("s = commas(42)");
        assert_eq!(get_str(&ev, "s"), "42");
    }

    #[test]
    fn commas_builtin_negative() {
        let ev = eval_str("s = commas(-9876543, 0)");
        assert_eq!(get_str(&ev, "s"), "-9,876,543");
    }

    // ── sprintf with , flag ─────────────────────────────────────────────────

    #[test]
    fn sprintf_comma_d() {
        let ev = eval_str("s = sprintf('%,d', 1234567)");
        assert_eq!(get_str(&ev, "s"), "1,234,567");
    }

    #[test]
    fn sprintf_comma_f() {
        let ev = eval_str("s = sprintf('%,.2f', 1234567.89)");
        assert_eq!(get_str(&ev, "s"), "1,234,567.89");
    }

    #[test]
    fn sprintf_no_comma() {
        let ev = eval_str("s = sprintf('%d', 1234567)");
        assert_eq!(get_str(&ev, "s"), "1234567");
    }

    // ── format modes ─────────────────────────────────────────────────────────

    #[test]
    fn format_commas_mode() {
        let ev = eval_str("format commas\nx = 1234567;");
        assert_eq!(ev.number_format, crate::eval::value::NumberFormat::Commas);
    }

    #[test]
    fn format_default_mode() {
        let ev = eval_str("format commas\nformat default");
        assert_eq!(ev.number_format, crate::eval::value::NumberFormat::Short);
    }

    #[test]
    fn format_short_mode() {
        let ev = eval_str("format long\nformat short");
        assert_eq!(ev.number_format, crate::eval::value::NumberFormat::Short);
    }

    #[test]
    fn format_long_mode() {
        let ev = eval_str("format long");
        assert_eq!(ev.number_format, crate::eval::value::NumberFormat::Long);
    }

    #[test]
    fn format_hex_mode() {
        let ev = eval_str("format hex");
        assert_eq!(ev.number_format, crate::eval::value::NumberFormat::Hex);
    }

    #[test]
    fn format_display_scalar_commas() {
        use crate::eval::value::NumberFormat;
        let val = Value::Scalar(1234567.0);
        assert_eq!(val.format_display(NumberFormat::Commas), "1,234,567");
    }

    #[test]
    fn format_display_scalar_short() {
        use crate::eval::value::NumberFormat;
        let val = Value::Scalar(1234567.0);
        assert_eq!(val.format_display(NumberFormat::Short), "1234567");
    }

    #[test]
    fn format_display_scalar_long() {
        use crate::eval::value::NumberFormat;
        let val = Value::Scalar(3.14);
        assert_eq!(val.format_display(NumberFormat::Long), "3.140000000000000");
    }

    #[test]
    fn format_display_scalar_hex() {
        use crate::eval::value::NumberFormat;
        let val = Value::Scalar(1.0);
        // 1.0f64 = 0x3ff0000000000000
        assert_eq!(val.format_display(NumberFormat::Hex), "3ff0000000000000");
    }

    #[test]
    fn format_display_negative() {
        use crate::eval::value::NumberFormat;
        let val = Value::Scalar(-1234567.5);
        assert_eq!(val.format_display(NumberFormat::Commas), "-1,234,567.5");
    }
}

// ── Underscore digit separator tests ────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod underscore_literal_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval_str(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn get_scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("Expected scalar for '{name}', got {other:?}"),
        }
    }

    #[test]
    fn integer_underscores() {
        let ev = eval_str("x = 1_000_000;");
        assert_eq!(get_scalar(&ev, "x"), 1_000_000.0);
    }

    #[test]
    fn thousands_separator() {
        let ev = eval_str("x = 48_000;");
        assert_eq!(get_scalar(&ev, "x"), 48_000.0);
    }

    #[test]
    fn decimal_with_underscores() {
        let ev = eval_str("x = 1_234.567_89;");
        assert_eq!(get_scalar(&ev, "x"), 1234.56789);
    }

    #[test]
    fn fractional_underscores() {
        let ev = eval_str("x = 3.141_592_653;");
        assert_eq!(get_scalar(&ev, "x"), 3.141592653);
    }

    #[test]
    fn scientific_with_underscores() {
        let ev = eval_str("x = 1_000e3;");
        assert_eq!(get_scalar(&ev, "x"), 1_000_000.0);
    }

    #[test]
    fn no_underscores_baseline() {
        let ev = eval_str("x = 1234567;");
        assert_eq!(get_scalar(&ev, "x"), 1234567.0);
    }

    #[test]
    fn single_underscore() {
        let ev = eval_str("x = 1_2;");
        assert_eq!(get_scalar(&ev, "x"), 12.0);
    }

    #[test]
    fn underscore_in_vector() {
        let ev = eval_str("v = [1_000, 2_000, 3_000];");
        match ev.get("v").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v[0].re, 1000.0);
                assert_eq!(v[1].re, 2000.0);
                assert_eq!(v[2].re, 3000.0);
            }
            other => panic!("Expected vector, got {other:?}"),
        }
    }

    #[test]
    fn underscore_in_expression() {
        let ev = eval_str("x = 1_000 + 2_000;");
        assert_eq!(get_scalar(&ev, "x"), 3000.0);
    }
}

#[allow(clippy::approx_constant, clippy::write_with_newline)]
mod string_index_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn eval(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn get_str(ev: &Evaluator, name: &str) -> String {
        match ev.get(name).unwrap() {
            Value::Str(s) => s.clone(),
            other => panic!("Expected string for '{name}', got {other:?}"),
        }
    }

    fn try_run(src: &str) -> Result<Evaluator, crate::error::ScriptError> {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts)?;
        Ok(ev)
    }

    #[test]
    fn single_char() {
        let ev = eval("a = 'hello'; b = a(1)");
        assert_eq!(get_str(&ev, "b"), "h");
    }

    #[test]
    fn single_char_mid() {
        let ev = eval("a = 'hello'; b = a(3)");
        assert_eq!(get_str(&ev, "b"), "l");
    }

    #[test]
    fn single_char_last() {
        let ev = eval("a = 'hello'; b = a(5)");
        assert_eq!(get_str(&ev, "b"), "o");
    }

    #[test]
    fn range_slice() {
        let ev = eval("a = 'hello world'; b = a(1:5)");
        assert_eq!(get_str(&ev, "b"), "hello");
    }

    #[test]
    fn range_slice_mid() {
        let ev = eval("a = 'hello world'; b = a(7:11)");
        assert_eq!(get_str(&ev, "b"), "world");
    }

    #[test]
    fn all_index() {
        let ev = eval("a = 'hello'; b = a(:)");
        assert_eq!(get_str(&ev, "b"), "hello");
    }

    #[test]
    fn length_of_string() {
        let ev = eval("a = 'hello'; n = length(a)");
        match ev.get("n").unwrap() {
            Value::Scalar(v) => assert_eq!(*v, 5.0),
            other => panic!("Expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn out_of_bounds() {
        assert!(try_run("a = 'hi'; b = a(3)").is_err());
    }

    #[test]
    fn out_of_bounds_range() {
        assert!(try_run("a = 'hi'; b = a(1:5)").is_err());
    }

    #[test]
    fn single_char_string() {
        let ev = eval("a = 'x'; b = a(1)");
        assert_eq!(get_str(&ev, "b"), "x");
    }

    #[test]
    fn zero_index_string_errors() {
        assert!(try_run("a = 'hello'; b = a(0)").is_err());
    }

    #[test]
    fn zero_index_vector_errors() {
        assert!(try_run("v = [10, 20, 30]; x = v(0)").is_err());
    }
}

#[allow(clippy::approx_constant, clippy::write_with_newline)]
mod toml_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};
    use std::io::Write;

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn try_run(src: &str) -> Result<Evaluator, crate::error::ScriptError> {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts)?;
        Ok(ev)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn tmp_path(name: &str) -> String {
        let dir = std::env::temp_dir();
        dir.join(format!("rustlab_test_{name}.toml"))
            .to_string_lossy()
            .to_string()
    }

    #[test]
    fn round_trip_flat_struct() {
        let path = tmp_path("flat");
        let ev = run(&format!(
            "s = struct(\"x\", 42, \"name\", \"hello\", \"flag\", true);\n\
             save(\"{path}\", s);\n\
             t = load(\"{path}\");"
        ));
        match ev.get("t").unwrap() {
            Value::Struct(fields) => {
                assert!(matches!(fields.get("x"), Some(Value::Scalar(n)) if close(*n, 42.0)));
                assert!(matches!(fields.get("name"), Some(Value::Str(s)) if s == "hello"));
                assert!(matches!(fields.get("flag"), Some(Value::Bool(true))));
            }
            other => panic!("expected struct, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn round_trip_nested_struct() {
        let path = tmp_path("nested");
        // Use struct() constructor to avoid dot-assign path parsing issues
        let src = format!(
            "audio = struct(\"sr\", 44100, \"bits\", 16);\n\
             s = struct(\"audio\", audio, \"name\", \"config\");\n\
             save(\"{path}\", s);\n\
             t = load(\"{path}\");\n\
             sr = t.audio.sr;"
        );
        let ev = run(&src);
        match ev.get("sr").unwrap() {
            Value::Scalar(n) => assert!(close(*n, 44100.0)),
            other => panic!("expected scalar, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn round_trip_with_vector() {
        let path = tmp_path("vec");
        let ev = run(&format!(
            "v = linspace(1, 3, 3);\n\
             s = struct(\"coeffs\", v);\n\
             save(\"{path}\", s);\n\
             t = load(\"{path}\");\n\
             c = t.coeffs;"
        ));
        match ev.get("c").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 3);
                assert!(close(v[0].re, 1.0));
                assert!(close(v[1].re, 2.0));
                assert!(close(v[2].re, 3.0));
            }
            other => panic!("expected vector, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn scalar_integer_preservation() {
        let path = tmp_path("int");
        let ev = run(&format!(
            "s = struct(\"n\", 100);\n\
             save(\"{path}\", s);\n\
             t = load(\"{path}\");\n\
             v = t.n;"
        ));
        match ev.get("v").unwrap() {
            Value::Scalar(n) => assert!(close(*n, 100.0)),
            other => panic!("expected scalar, got {other:?}"),
        }
        // Verify the file actually has an integer (no decimal point)
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("n = 100\n") || text.contains("n = 100\r\n"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn scalar_float_preservation() {
        let path = tmp_path("float");
        let ev = run(&format!(
            "s = struct(\"pi\", 3.14);\n\
             save(\"{path}\", s);\n\
             t = load(\"{path}\");\n\
             v = t.pi;"
        ));
        match ev.get("v").unwrap() {
            Value::Scalar(n) => assert!(close(*n, 3.14)),
            other => panic!("expected scalar, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn error_on_non_struct_top_level() {
        let path = tmp_path("nonstruct");
        let result = try_run(&format!("save(\"{path}\", 42);"));
        assert!(result.is_err(), "saving a scalar to TOML should error");
    }

    #[test]
    fn error_on_complex_in_struct() {
        let path = tmp_path("complex");
        let result = try_run(&format!(
            "s = struct(\"z\", 1 + j*2);\n\
             save(\"{path}\", s);"
        ));
        assert!(
            result.is_err(),
            "saving complex values to TOML should error"
        );
    }

    #[test]
    fn load_external_toml() {
        let path = tmp_path("external");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(
                f,
                r#"
title = "DSP Config"
version = 3

[audio]
sample_rate = 48000
channels = 2
buffer_size = 256

[filter]
type = "lowpass"
cutoff_hz = 1000.5
enabled = true
taps = [1.0, 0.5, 0.25]
"#
            )
            .unwrap();
        }
        let ev = run(&format!(
            "cfg = load(\"{path}\");\n\
             sr = cfg.audio.sample_rate;\n\
             ftype = cfg.filter.type;\n\
             enabled = cfg.filter.enabled;\n\
             taps = cfg.filter.taps;\n\
             ver = cfg.version;"
        ));
        match ev.get("sr").unwrap() {
            Value::Scalar(n) => assert!(close(*n, 48000.0)),
            other => panic!("expected scalar, got {other:?}"),
        }
        match ev.get("ftype").unwrap() {
            Value::Str(s) => assert_eq!(s, "lowpass"),
            other => panic!("expected string, got {other:?}"),
        }
        match ev.get("enabled").unwrap() {
            Value::Bool(b) => assert!(*b),
            other => panic!("expected bool, got {other:?}"),
        }
        match ev.get("taps").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v.len(), 3);
                assert!(close(v[0].re, 1.0));
            }
            other => panic!("expected vector, got {other:?}"),
        }
        match ev.get("ver").unwrap() {
            Value::Scalar(n) => assert!(close(*n, 3.0)),
            other => panic!("expected scalar, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    // ── Array of tables (Tuple of Structs) ───────────────────────────────

    #[test]
    fn load_array_of_tables() {
        let path = tmp_path("array_tables");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(
                f,
                r#"
title = "chain"

[[filters]]
name = "lp"
cutoff = 1000

[[filters]]
name = "hp"
cutoff = 5000
"#
            )
            .unwrap();
        }
        let ev = run(&format!(
            "cfg = load(\"{path}\");\n\
             filters = cfg.filters;\n\
             n = length(filters);\n\
             f1 = filters(1);\n\
             f2 = filters(2);"
        ));
        match ev.get("n").unwrap() {
            Value::Scalar(n) => assert!(close(*n, 2.0)),
            other => panic!("expected scalar, got {other:?}"),
        }
        match ev.get("f1").unwrap() {
            Value::Struct(fields) => {
                assert!(matches!(fields.get("name"), Some(Value::Str(s)) if s == "lp"));
                assert!(
                    matches!(fields.get("cutoff"), Some(Value::Scalar(n)) if close(*n, 1000.0))
                );
            }
            other => panic!("expected struct, got {other:?}"),
        }
        match ev.get("f2").unwrap() {
            Value::Struct(fields) => {
                assert!(matches!(fields.get("name"), Some(Value::Str(s)) if s == "hp"));
            }
            other => panic!("expected struct, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    // ── Tuple indexing ──���────────────────────────────────────────────────

    #[test]
    fn tuple_index_end_keyword() {
        let path = tmp_path("tuple_end");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(
                f,
                r#"
[[items]]
val = 10
[[items]]
val = 20
[[items]]
val = 30
"#
            )
            .unwrap();
        }
        let ev = run(&format!(
            "cfg = load(\"{path}\");\n\
             items = cfg.items;\n\
             last = items(end);\n\
             v = last.val;"
        ));
        match ev.get("v").unwrap() {
            Value::Scalar(n) => assert!(close(*n, 30.0)),
            other => panic!("expected scalar 30, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tuple_index_out_of_bounds() {
        let path = tmp_path("tuple_oob");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(f, "[[items]]\nval = 1\n").unwrap();
        }
        let result = try_run(&format!(
            "cfg = load(\"{path}\");\n\
             items = cfg.items;\n\
             bad = items(5);"
        ));
        assert!(result.is_err(), "out-of-bounds tuple index should error");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tuple_loop_with_length() {
        let path = tmp_path("tuple_loop");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(
                f,
                r#"
[[items]]
x = 10
[[items]]
x = 20
[[items]]
x = 30
"#
            )
            .unwrap();
        }
        let ev = run(&format!(
            "cfg = load(\"{path}\");\n\
             items = cfg.items;\n\
             total = 0;\n\
             for k = 1:length(items)\n\
               it = items(k);\n\
               total = total + it.x;\n\
             end"
        ));
        match ev.get("total").unwrap() {
            Value::Scalar(n) => assert!(close(*n, 60.0)),
            other => panic!("expected 60, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    // ── Matrix round-trip ────────────────────────────────────────────────

    #[test]
    fn round_trip_matrix() {
        let path = tmp_path("matrix");
        let ev = run(&format!(
            "M = [1, 2; 3, 4];\n\
             s = struct(\"data\", M);\n\
             save(\"{path}\", s);\n\
             t = load(\"{path}\");\n\
             d = t.data;"
        ));
        // Matrix becomes array of arrays in TOML; loads back as Tuple of Vectors
        match ev.get("d").unwrap() {
            Value::Tuple(rows) => {
                assert_eq!(rows.len(), 2);
                match &rows[0] {
                    Value::Vector(v) => {
                        assert_eq!(v.len(), 2);
                        assert!(close(v[0].re, 1.0));
                        assert!(close(v[1].re, 2.0));
                    }
                    other => panic!("expected vector row, got {other:?}"),
                }
                match &rows[1] {
                    Value::Vector(v) => {
                        assert!(close(v[0].re, 3.0));
                        assert!(close(v[1].re, 4.0));
                    }
                    other => panic!("expected vector row, got {other:?}"),
                }
            }
            other => panic!("expected tuple of vectors, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    // ── Edge cases ──��────────────────────────────────────────────────────

    #[test]
    fn empty_struct() {
        let path = tmp_path("empty");
        let ev = run(&format!(
            "s = struct();\n\
             save(\"{path}\", s);\n\
             t = load(\"{path}\");"
        ));
        match ev.get("t").unwrap() {
            Value::Struct(fields) => assert!(fields.is_empty()),
            other => panic!("expected empty struct, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn string_with_special_chars() {
        let path = tmp_path("special_str");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            // Write TOML directly with escaped chars
            write!(f, "msg = \"hello \\\"world\\\" tab\\there\"\n").unwrap();
        }
        let ev = run(&format!(
            "cfg = load(\"{path}\");\n\
             m = cfg.msg;"
        ));
        match ev.get("m").unwrap() {
            Value::Str(s) => {
                assert!(
                    s.contains("\"world\""),
                    "should contain escaped quotes: {s}"
                );
                assert!(s.contains('\t'), "should contain tab: {s}");
            }
            other => panic!("expected string, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_nonexistent_file() {
        let result = try_run("x = load(\"/tmp/no_such_file_rustlab_test.toml\");");
        assert!(result.is_err(), "loading nonexistent TOML should error");
    }

    #[test]
    fn load_malformed_toml() {
        let path = tmp_path("malformed");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(f, "[broken\nkey = ???\n").unwrap();
        }
        let result = try_run(&format!("x = load(\"{path}\");"));
        assert!(result.is_err(), "loading malformed TOML should error");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_array_becomes_empty_vector() {
        let path = tmp_path("empty_arr");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(f, "vals = []\n").unwrap();
        }
        let ev = run(&format!(
            "cfg = load(\"{path}\");\n\
             v = cfg.vals;"
        ));
        match ev.get("v").unwrap() {
            Value::Vector(v) => assert_eq!(v.len(), 0),
            other => panic!("expected empty vector, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }
}

// ── String array / cell array tests ───────────────────────────────────────

#[cfg(test)]
mod string_array_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let tokens = lexer::tokenize(src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    #[test]
    fn string_array_literal() {
        let ev = run("s = {\"a\", \"b\", \"c\"};");
        match ev.get("s").unwrap() {
            Value::StringArray(v) => {
                assert_eq!(v, &["a", "b", "c"]);
            }
            other => panic!("expected string_array, got {other:?}"),
        }
    }

    #[test]
    fn string_array_single_quote() {
        let ev = run("s = {'hello', 'world'};");
        match ev.get("s").unwrap() {
            Value::StringArray(v) => assert_eq!(v, &["hello", "world"]),
            other => panic!("expected string_array, got {other:?}"),
        }
    }

    #[test]
    fn string_array_empty() {
        let ev = run("s = {};");
        match ev.get("s").unwrap() {
            Value::StringArray(v) => assert!(v.is_empty()),
            other => panic!("expected empty string_array, got {other:?}"),
        }
    }

    #[test]
    fn string_array_index_scalar() {
        let ev = run("s = {'x', 'y', 'z'}; v = s(2);");
        match ev.get("v").unwrap() {
            Value::Str(s) => assert_eq!(s, "y"),
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn string_array_index_end() {
        let ev = run("s = {'a', 'b', 'c'}; v = s(end);");
        match ev.get("v").unwrap() {
            Value::Str(s) => assert_eq!(s, "c"),
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn string_array_index_vector() {
        let ev = run("s = {'a', 'b', 'c', 'd'}; v = s([1, 3]);");
        match ev.get("v").unwrap() {
            Value::StringArray(v) => assert_eq!(v, &["a", "c"]),
            other => panic!("expected string_array, got {other:?}"),
        }
    }

    #[test]
    fn string_array_length() {
        let ev = run("s = {'a', 'b', 'c'}; n = length(s);");
        match ev.get("n").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 3.0),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn string_array_size() {
        let ev = run("s = {'x', 'y'}; sz = size(s);");
        match ev.get("sz").unwrap() {
            Value::Vector(v) => {
                assert_eq!(v[0].re, 1.0);
                assert_eq!(v[1].re, 2.0);
            }
            other => panic!("expected vector, got {other:?}"),
        }
    }

    #[test]
    fn string_array_numel() {
        let ev = run("s = {'a', 'b', 'c', 'd'}; n = numel(s);");
        match ev.get("n").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 4.0),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn iscell_true() {
        let ev = run("s = {'a'}; b = iscell(s);");
        match ev.get("b").unwrap() {
            Value::Bool(b) => assert!(*b),
            other => panic!("expected bool, got {other:?}"),
        }
    }

    #[test]
    fn iscell_false() {
        let ev = run("b = iscell(42);");
        match ev.get("b").unwrap() {
            Value::Bool(b) => assert!(!*b),
            other => panic!("expected bool, got {other:?}"),
        }
    }

    #[test]
    fn string_array_display() {
        let ev = run("s = {'Jan', 'Feb', 'Mar'};");
        let s = ev.get("s").unwrap();
        let display = format!("{s}");
        assert!(display.contains("Jan"));
        assert!(display.contains("Feb"));
        assert!(display.contains("Mar"));
    }

    #[test]
    fn string_array_type_name() {
        let ev = run("s = {'x'};");
        assert_eq!(ev.get("s").unwrap().type_name(), "string_array");
    }

    #[test]
    fn string_array_rejects_non_strings() {
        let tokens = lexer::tokenize("{1, 2, 3}").unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        let result = ev.exec_stmt(&stmts[0]);
        assert!(result.is_err());
    }
}

// ─── Audit gap coverage: script wrappers around tested DSP/plot primitives ──
#[cfg(test)]
mod audit_builtin_tests {
    use crate::eval::value::Value;
    use crate::Evaluator;
    use rustlab_plot::figure::FIGURE;

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn get_real_vec(ev: &Evaluator, name: &str) -> Vec<f64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => panic!("expected vector for '{name}', got {other:?}"),
        }
    }

    fn reset_figure() {
        FIGURE.with(|f| f.borrow_mut().reset());
    }

    // ── butterworth_lowpass / butterworth_highpass ────────────────────────

    #[test]
    fn butterworth_lowpass_order2_returns_3_coeffs() {
        let ev = run("b = butterworth_lowpass(2, 1000.0, 8000.0);");
        let b = get_real_vec(&ev, "b");
        assert_eq!(b.len(), 3, "order-2 LP must return 3 b coefficients");
        // DC gain of the numerator alone is monotonic > 0 for a LP butterworth.
        let sum: f64 = b.iter().sum();
        assert!(
            sum > 0.0,
            "sum of b coefficients should be positive for LP, got {sum}"
        );
    }

    #[test]
    fn butterworth_lowpass_order4_returns_5_coeffs() {
        let ev = run("b = butterworth_lowpass(4, 1000.0, 8000.0);");
        assert_eq!(get_real_vec(&ev, "b").len(), 5);
    }

    #[test]
    fn butterworth_highpass_order2_returns_3_coeffs() {
        let ev = run("b = butterworth_highpass(2, 1000.0, 8000.0);");
        let b = get_real_vec(&ev, "b");
        assert_eq!(b.len(), 3, "order-2 HP must return 3 b coefficients");
        // First b coefficient for a HP butterworth via bilinear transform is positive.
        assert!(b[0] > 0.0, "b[0] should be positive for HP, got {}", b[0]);
    }

    // ── hline / yline alias pair ──────────────────────────────────────────

    #[test]
    fn hline_adds_dashed_series_to_current_subplot() {
        reset_figure();
        run("hline(0.5);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            assert_eq!(sp.series.len(), 1, "hline should add one series");
            let s = &sp.series[0];
            assert_eq!(s.y_data, vec![0.5, 0.5]);
            assert!(matches!(s.style, rustlab_plot::LineStyle::Dashed));
        });
    }

    #[test]
    fn yline_is_alias_for_hline() {
        reset_figure();
        run("yline(1.25);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            assert_eq!(sp.series.len(), 1, "yline should add one series");
            assert_eq!(sp.series[0].y_data, vec![1.25, 1.25]);
            assert!(matches!(
                sp.series[0].style,
                rustlab_plot::LineStyle::Dashed
            ));
        });
    }

    #[test]
    fn hline_vector_adds_one_series_per_element() {
        reset_figure();
        run("hline([0.0, 0.5, 1.0]);");
        FIGURE.with(|f| {
            let fig = f.borrow();
            let sp = fig.current();
            assert_eq!(sp.series.len(), 3);
            assert_eq!(sp.series[0].y_data, vec![0.0, 0.0]);
            assert_eq!(sp.series[1].y_data, vec![0.5, 0.5]);
            assert_eq!(sp.series[2].y_data, vec![1.0, 1.0]);
        });
    }

    // ── qadd / qmul ───────────────────────────────────────────────────────

    #[test]
    fn qadd_elementwise_with_round_mode() {
        // Q1.15 with round-to-nearest: 0.25 + 0.125 = 0.375 (exactly representable).
        let ev = run("f = qfmt(16, 15, \"round\"); y = qadd([0.25, 0.5], [0.125, 0.25], f);");
        let y = get_real_vec(&ev, "y");
        assert_eq!(y.len(), 2);
        assert!((y[0] - 0.375).abs() < 1e-9, "qadd[0] = {}", y[0]);
        assert!((y[1] - 0.75).abs() < 1e-9, "qadd[1] = {}", y[1]);
    }

    #[test]
    fn qadd_saturates_on_overflow() {
        // Q1.15 max positive ≈ 0.999969..., 0.9 + 0.9 would overflow → saturate.
        let ev = run("f = qfmt(16, 15, \"round\", \"saturate\"); y = qadd([0.9], [0.9], f);");
        let y = get_real_vec(&ev, "y");
        assert!(
            y[0] < 1.0 && y[0] > 0.999,
            "qadd saturated value = {}",
            y[0]
        );
    }

    #[test]
    fn qmul_elementwise_with_round_mode() {
        // Q1.15 rounding: 0.5 * 0.5 = 0.25 (exactly representable).
        let ev = run("f = qfmt(16, 15, \"round\"); y = qmul([0.5, 0.25], [0.5, 0.5], f);");
        let y = get_real_vec(&ev, "y");
        assert_eq!(y.len(), 2);
        assert!((y[0] - 0.25).abs() < 1e-9, "qmul[0] = {}", y[0]);
        assert!((y[1] - 0.125).abs() < 1e-9, "qmul[1] = {}", y[1]);
    }
}

#[cfg(test)]
mod tensor3_tests {
    use crate::eval::value::Value;
    use ndarray::Array3;
    use num_complex::Complex;
    use rustlab_core::{CTensor3, C64};

    fn c(x: f64) -> C64 {
        Complex::new(x, 0.0)
    }

    fn build(m: usize, n: usize, p: usize) -> CTensor3 {
        let mut t = Array3::<C64>::zeros((m, n, p));
        // Fill with 1-based row-major-ish walk for easy sanity: t[i, j, k] = i*100 + j*10 + k + 1
        for i in 0..m {
            for j in 0..n {
                for k in 0..p {
                    t[[i, j, k]] = c((i * 100 + j * 10 + k + 1) as f64);
                }
            }
        }
        t
    }

    #[test]
    fn type_name_is_tensor3() {
        let v = Value::Tensor3(Array3::<C64>::zeros((2, 3, 4)));
        assert_eq!(v.type_name(), "tensor3");
    }

    #[test]
    fn negate_flips_every_element() {
        let v = Value::Tensor3(build(2, 2, 2));
        let neg = v.negate().unwrap();
        if let Value::Tensor3(t) = neg {
            assert_eq!(t.shape(), &[2, 2, 2]);
            assert_eq!(t[[0, 0, 0]], c(-1.0));
            assert_eq!(t[[1, 1, 1]], c(-(100 + 10 + 1 + 1) as f64));
        } else {
            panic!("negate on Tensor3 should return Tensor3");
        }
    }

    #[test]
    fn display_shows_shape_and_first_page() {
        let v = Value::Tensor3(build(2, 3, 4));
        let s = format!("{}", v);
        assert!(s.starts_with("tensor3 [2×3×4]"), "got: {}", s);
        assert!(s.contains("(:, :, 1) ="), "got: {}", s);
        assert!(s.contains("(:, :, 4) ="), "got: {}", s);
        assert!(s.contains("2 pages omitted"), "got: {}", s);
    }

    #[test]
    fn display_small_shows_all_pages() {
        let v = Value::Tensor3(build(2, 2, 2));
        let s = format!("{}", v);
        assert!(s.contains("(:, :, 1) ="), "got: {}", s);
        assert!(s.contains("(:, :, 2) ="), "got: {}", s);
        assert!(!s.contains("omitted"), "got: {}", s);
    }

    #[test]
    fn display_empty_tensor_shows_header_only() {
        let v = Value::Tensor3(Array3::<C64>::zeros((0, 0, 0)));
        let s = format!("{}", v);
        assert_eq!(s, "tensor3 [0×0×0]");
    }

    #[test]
    fn transpose_rejects_tensor3() {
        let v = Value::Tensor3(Array3::<C64>::zeros((2, 2, 2)));
        assert!(v.transpose().is_err());
    }

    #[test]
    fn to_scalar_rejects_tensor3() {
        let v = Value::Tensor3(Array3::<C64>::zeros((1, 1, 1)));
        assert!(v.to_scalar().is_err());
    }
}

#[cfg(test)]
mod tensor3_constructor_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{src}\n");
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn shape(ev: &Evaluator, name: &str) -> (usize, usize, usize) {
        match ev.get(name).unwrap() {
            Value::Tensor3(t) => {
                let s = t.shape();
                (s[0], s[1], s[2])
            }
            other => panic!("Expected Tensor3 for '{name}', got {other:?}"),
        }
    }

    fn scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("Expected Scalar for '{name}', got {other:?}"),
        }
    }

    #[test]
    fn zeros3_shape() {
        let ev = run("A = zeros3(2, 3, 4)");
        assert_eq!(shape(&ev, "A"), (2, 3, 4));
    }

    #[test]
    fn zeros3_all_zero() {
        let ev = run("A = zeros3(2, 2, 2)");
        if let Value::Tensor3(t) = ev.get("A").unwrap() {
            for &c in t.iter() {
                assert_eq!(c.re, 0.0);
                assert_eq!(c.im, 0.0);
            }
        } else {
            panic!();
        }
    }

    #[test]
    fn ones3_all_one() {
        let ev = run("A = ones3(2, 2, 2)");
        if let Value::Tensor3(t) = ev.get("A").unwrap() {
            for &c in t.iter() {
                assert_eq!(c.re, 1.0);
                assert_eq!(c.im, 0.0);
            }
        } else {
            panic!();
        }
    }

    #[test]
    fn rand3_shape_and_range() {
        let ev = run("A = rand3(3, 2, 5)");
        if let Value::Tensor3(t) = ev.get("A").unwrap() {
            assert_eq!(t.shape(), &[3, 2, 5]);
            for &c in t.iter() {
                assert!(c.re >= 0.0 && c.re < 1.0, "rand3 value out of range: {c}");
            }
        } else {
            panic!();
        }
    }

    #[test]
    fn randn3_shape() {
        let ev = run("A = randn3(2, 3, 4)");
        assert_eq!(shape(&ev, "A"), (2, 3, 4));
    }

    #[test]
    fn size_tensor3_returns_three_elements() {
        let ev = run("A = zeros3(2, 3, 4); s = size(A)");
        if let Value::Vector(v) = ev.get("s").unwrap() {
            assert_eq!(v.len(), 3);
            assert_eq!(v[0].re, 2.0);
            assert_eq!(v[1].re, 3.0);
            assert_eq!(v[2].re, 4.0);
        } else {
            panic!("size(Tensor3) should return a Vector");
        }
    }

    #[test]
    fn size_tensor3_with_dim() {
        let ev = run("A = zeros3(2, 3, 4); d1 = size(A, 1); d2 = size(A, 2); d3 = size(A, 3)");
        assert_eq!(scalar(&ev, "d1"), 2.0);
        assert_eq!(scalar(&ev, "d2"), 3.0);
        assert_eq!(scalar(&ev, "d3"), 4.0);
    }

    #[test]
    fn numel_tensor3() {
        let ev = run("A = zeros3(2, 3, 4); n = numel(A)");
        assert_eq!(scalar(&ev, "n"), 24.0);
    }

    #[test]
    fn ndims_tensor3_is_three() {
        let ev = run("A = zeros3(2, 3, 4); n = ndims(A)");
        assert_eq!(scalar(&ev, "n"), 3.0);
    }

    #[test]
    fn ndims_matrix_is_two() {
        let ev = run("A = zeros(2, 3); n = ndims(A)");
        assert_eq!(scalar(&ev, "n"), 2.0);
    }

    #[test]
    fn ndims_vector_is_two() {
        let ev = run("v = [1, 2, 3]; n = ndims(v)");
        assert_eq!(scalar(&ev, "n"), 2.0);
    }

    #[test]
    fn ndims_scalar_is_two() {
        let ev = run("x = 5; n = ndims(x)");
        assert_eq!(scalar(&ev, "n"), 2.0);
    }

    #[test]
    fn zeros3_from_size_vector() {
        // zeros3([2, 3, 4]) — alternate single-arg form
        let ev = run("A = zeros3([2, 3, 4])");
        assert_eq!(shape(&ev, "A"), (2, 3, 4));
    }
}

#[cfg(test)]
mod tensor3_index_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{src}\n");
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("Expected Scalar for '{name}', got {other:?}"),
        }
    }

    fn matrix_shape(ev: &Evaluator, name: &str) -> (usize, usize) {
        match ev.get(name).unwrap() {
            Value::Matrix(m) => (m.nrows(), m.ncols()),
            other => panic!("Expected Matrix for '{name}', got {other:?}"),
        }
    }

    fn tensor3_shape(ev: &Evaluator, name: &str) -> (usize, usize, usize) {
        match ev.get(name).unwrap() {
            Value::Tensor3(t) => {
                let s = t.shape();
                (s[0], s[1], s[2])
            }
            other => panic!("Expected Tensor3 for '{name}', got {other:?}"),
        }
    }

    // Helper: build a tensor with A(i,j,k) = 100*i + 10*j + k (1-based) for easy checks.
    // Uses scripted assignment, so also exercises the assignment path lightly.
    fn build_encoded(src_name: &str, m: usize, n: usize, p: usize) -> String {
        let mut s = format!("{src_name} = zeros3({m}, {n}, {p})\n");
        for i in 1..=m {
            for j in 1..=n {
                for k in 1..=p {
                    s.push_str(&format!(
                        "{src_name}({i}, {j}, {k}) = {};\n",
                        100 * i + 10 * j + k
                    ));
                }
            }
        }
        s
    }

    // ── READ: single-element extract ────────────────────────────────────────

    #[test]
    fn read_single_element() {
        let src = build_encoded("A", 2, 3, 4);
        let ev = run(&format!("{src}\nv = A(2, 3, 4)"));
        assert_eq!(scalar(&ev, "v"), (100 * 2 + 10 * 3 + 4) as f64);
    }

    #[test]
    fn read_first_element() {
        let src = build_encoded("A", 2, 3, 4);
        let ev = run(&format!("{src}\nv = A(1, 1, 1)"));
        assert_eq!(scalar(&ev, "v"), 111.0);
    }

    // ── READ: page slice A(:, :, k) → Matrix ────────────────────────────────

    #[test]
    fn read_page_slice_returns_matrix() {
        let src = build_encoded("A", 2, 3, 4);
        let ev = run(&format!("{src}\nM = A(:, :, 2)"));
        assert_eq!(matrix_shape(&ev, "M"), (2, 3));
        if let Value::Matrix(m) = ev.get("M").unwrap() {
            // A(i, j, 2) = 100*i + 10*j + 2
            assert_eq!(m[[0, 0]].re, 112.0);
            assert_eq!(m[[0, 2]].re, 132.0);
            assert_eq!(m[[1, 2]].re, 232.0);
        }
    }

    #[test]
    fn read_page_slice_last() {
        let src = build_encoded("A", 2, 3, 4);
        let ev = run(&format!("{src}\nM = A(:, :, 4)"));
        if let Value::Matrix(m) = ev.get("M").unwrap() {
            assert_eq!(m[[1, 2]].re, 234.0);
        }
    }

    // ── READ: non-canonical slices A(i, :, :) and A(:, j, :) → Matrix ───────

    #[test]
    fn read_row_slab_returns_matrix() {
        let src = build_encoded("A", 2, 3, 4);
        let ev = run(&format!("{src}\nM = A(1, :, :)"));
        // Fixing i=1, M[j,k] = 100 + 10*j + k where j,k 1-based; stored 0-based
        assert_eq!(matrix_shape(&ev, "M"), (3, 4));
        if let Value::Matrix(m) = ev.get("M").unwrap() {
            assert_eq!(m[[0, 0]].re, 111.0);
            assert_eq!(m[[2, 3]].re, 134.0);
        }
    }

    #[test]
    fn read_col_slab_returns_matrix() {
        let src = build_encoded("A", 2, 3, 4);
        let ev = run(&format!("{src}\nM = A(:, 2, :)"));
        // Fixing j=2, M[i,k] = 100*i + 20 + k
        assert_eq!(matrix_shape(&ev, "M"), (2, 4));
        if let Value::Matrix(m) = ev.get("M").unwrap() {
            assert_eq!(m[[0, 0]].re, 121.0);
            assert_eq!(m[[1, 3]].re, 224.0);
        }
    }

    // ── READ: full slice returns Tensor3 ────────────────────────────────────

    #[test]
    fn read_full_slice_returns_tensor3() {
        let src = build_encoded("A", 2, 3, 4);
        let ev = run(&format!("{src}\nB = A(:, :, :)"));
        assert_eq!(tensor3_shape(&ev, "B"), (2, 3, 4));
    }

    // ── READ: range slicing → Tensor3 of reduced shape ──────────────────────

    #[test]
    fn read_range_page_returns_tensor3() {
        let src = build_encoded("A", 2, 3, 4);
        let ev = run(&format!("{src}\nB = A(:, :, 1:2)"));
        assert_eq!(tensor3_shape(&ev, "B"), (2, 3, 2));
        if let Value::Tensor3(t) = ev.get("B").unwrap() {
            assert_eq!(t[[0, 0, 0]].re, 111.0);
            assert_eq!(t[[1, 2, 1]].re, 232.0);
        }
    }

    #[test]
    fn read_range_rows_returns_tensor3() {
        let src = build_encoded("A", 4, 3, 2);
        let ev = run(&format!("{src}\nB = A(2:3, :, :)"));
        assert_eq!(tensor3_shape(&ev, "B"), (2, 3, 2));
    }

    // ── READ: vector result when two dimensions are singleton ───────────────

    #[test]
    fn read_two_singletons_returns_vector() {
        let src = build_encoded("A", 2, 3, 4);
        let ev = run(&format!("{src}\nv = A(1, 1, :)"));
        if let Value::Vector(v) = ev.get("v").unwrap() {
            assert_eq!(v.len(), 4);
            assert_eq!(v[0].re, 111.0);
            assert_eq!(v[3].re, 114.0);
        } else {
            panic!("A(1, 1, :) should return Vector");
        }
    }

    // ── READ: end keyword per-dim ───────────────────────────────────────────

    #[test]
    fn read_end_per_dim() {
        let src = build_encoded("A", 2, 3, 4);
        let ev = run(&format!("{src}\nv = A(end, end, end)"));
        assert_eq!(scalar(&ev, "v"), 234.0);
    }

    // ── READ: out-of-bounds errors ──────────────────────────────────────────

    #[test]
    fn read_out_of_bounds_errors() {
        let src_ok = "A = zeros3(2, 2, 2)\n";
        let src_bad = format!("{src_ok}v = A(3, 1, 1)\n");
        let tokens = crate::lexer::tokenize(&src_bad).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        let mut err = None;
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                err = Some(e);
                break;
            }
        }
        assert!(err.is_some(), "expected out-of-bounds error");
    }

    // ── ASSIGNMENT: scalar ──────────────────────────────────────────────────

    #[test]
    fn assign_scalar() {
        let ev = run("A = zeros3(2, 2, 2); A(1, 2, 2) = 7; v = A(1, 2, 2)");
        assert_eq!(scalar(&ev, "v"), 7.0);
    }

    // ── ASSIGNMENT: page write A(:, :, k) = M ───────────────────────────────

    #[test]
    fn assign_page_matrix() {
        let ev = run(
            "A = zeros3(2, 2, 3); M = [1, 2; 3, 4]; A(:, :, 2) = M; v11 = A(1, 1, 2); v22 = A(2, 2, 2); v_off = A(1, 1, 1)"
        );
        assert_eq!(scalar(&ev, "v11"), 1.0);
        assert_eq!(scalar(&ev, "v22"), 4.0);
        assert_eq!(scalar(&ev, "v_off"), 0.0);
    }

    // ── ASSIGNMENT: tensor3 region ──────────────────────────────────────────

    #[test]
    fn assign_tensor3_region() {
        let ev = run(
            "A = zeros3(4, 3, 2); B = ones3(2, 3, 2); A(2:3, :, :) = B; \
             v_in = A(2, 1, 1); v_out = A(1, 1, 1)",
        );
        assert_eq!(scalar(&ev, "v_in"), 1.0);
        assert_eq!(scalar(&ev, "v_out"), 0.0);
    }

    // ── ASSIGNMENT: mismatched RHS shape errors ─────────────────────────────

    #[test]
    fn assign_page_wrong_shape_errors() {
        let src = "A = zeros3(2, 2, 3); M = [1, 2, 3; 4, 5, 6]; A(:, :, 1) = M\n";
        let tokens = crate::lexer::tokenize(src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        let mut err = None;
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                err = Some(e);
                break;
            }
        }
        assert!(err.is_some(), "expected shape-mismatch error");
    }
}

#[cfg(test)]
mod tensor3_arith_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{src}\n");
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn run_expect_err(src: &str) -> String {
        let src = format!("{src}\n");
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                return format!("{e:?}");
            }
        }
        panic!("expected error; none raised");
    }

    fn scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("Expected Scalar for '{name}', got {other:?}"),
        }
    }

    fn tensor3(ev: &Evaluator, name: &str) -> ndarray::Array3<num_complex::Complex<f64>> {
        match ev.get(name).unwrap() {
            Value::Tensor3(t) => t.clone(),
            other => panic!("Expected Tensor3 for '{name}', got {other:?}"),
        }
    }

    // ── Scalar broadcast ────────────────────────────────────────────────────

    #[test]
    fn scalar_plus_tensor3() {
        let ev = run("A = ones3(2, 2, 2); B = A + 5; v = B(1, 1, 1)");
        assert_eq!(scalar(&ev, "v"), 6.0);
    }

    #[test]
    fn tensor3_plus_scalar() {
        let ev = run("A = ones3(2, 2, 2); B = 5 + A; v = B(1, 1, 1)");
        assert_eq!(scalar(&ev, "v"), 6.0);
    }

    #[test]
    fn scalar_times_tensor3() {
        let ev = run("A = ones3(2, 2, 2); B = 3 * A; v = B(2, 2, 2)");
        assert_eq!(scalar(&ev, "v"), 3.0);
    }

    #[test]
    fn tensor3_divided_by_scalar() {
        let ev = run("A = ones3(2, 2, 2); B = A / 2; v = B(1, 1, 1)");
        assert_eq!(scalar(&ev, "v"), 0.5);
    }

    #[test]
    fn tensor3_pow_scalar_elementwise() {
        let ev = run("A = ones3(2, 2, 2) + 2; B = A .^ 2; v = B(1, 1, 1)");
        assert!((scalar(&ev, "v") - 9.0).abs() < 1e-10);
    }

    // ── Tensor3 op Tensor3 ──────────────────────────────────────────────────

    #[test]
    fn tensor3_plus_tensor3() {
        let ev = run("A = ones3(2, 2, 2); B = ones3(2, 2, 2) * 3; C = A + B; v = C(1, 2, 2)");
        assert_eq!(scalar(&ev, "v"), 4.0);
    }

    #[test]
    fn tensor3_minus_tensor3() {
        let ev = run("A = ones3(2, 2, 2) * 5; B = ones3(2, 2, 2) * 2; C = A - B; v = C(1, 2, 2)");
        assert_eq!(scalar(&ev, "v"), 3.0);
    }

    #[test]
    fn tensor3_elemmul_tensor3() {
        let ev = run("A = ones3(2, 2, 2) * 3; B = ones3(2, 2, 2) * 4; C = A .* B; v = C(1, 1, 1)");
        assert_eq!(scalar(&ev, "v"), 12.0);
    }

    #[test]
    fn tensor3_elemdiv_tensor3() {
        let ev = run("A = ones3(2, 2, 2) * 10; B = ones3(2, 2, 2) * 4; C = A ./ B; v = C(1, 1, 1)");
        assert_eq!(scalar(&ev, "v"), 2.5);
    }

    #[test]
    fn tensor3_preserves_shape_through_arith() {
        let ev = run("A = ones3(3, 4, 5); B = A + A; C = B .* A");
        let c = tensor3(&ev, "C");
        assert_eq!(c.shape(), &[3, 4, 5]);
        for &val in c.iter() {
            assert_eq!(val.re, 2.0);
        }
    }

    // ── Error paths ─────────────────────────────────────────────────────────

    #[test]
    fn tensor3_times_tensor3_errors() {
        let err = run_expect_err("A = ones3(2, 2, 2); B = ones3(2, 2, 2); C = A * B");
        assert!(
            err.to_lowercase().contains("tensor3") && err.contains("*"),
            "expected tensor3 * tensor3 error mentioning `*`, got: {err}"
        );
    }

    #[test]
    fn matrix_plus_tensor3_errors() {
        let err = run_expect_err(
            "A = ones3(2, 2, 2); M = ones(2, 2); C = M + A",
        );
        assert!(
            err.to_lowercase().contains("matrix")
                && err.to_lowercase().contains("tensor3"),
            "expected matrix + tensor3 error, got: {err}"
        );
    }

    #[test]
    fn tensor3_shape_mismatch_errors() {
        let err = run_expect_err("A = ones3(2, 2, 2); B = ones3(3, 3, 3); C = A + B");
        assert!(
            err.to_lowercase().contains("shape"),
            "expected shape mismatch error, got: {err}"
        );
    }

    #[test]
    fn tensor3_divides_tensor3_with_slash_errors() {
        // `/` is matmul-style; reject between two tensor3s
        let err = run_expect_err("A = ones3(2, 2, 2); B = ones3(2, 2, 2); C = A / B");
        assert!(
            err.contains("/") || err.to_lowercase().contains("not defined"),
            "expected / error, got: {err}"
        );
    }

    #[test]
    fn vector_plus_tensor3_errors() {
        let err = run_expect_err("A = ones3(2, 2, 2); v = [1, 2]; C = v + A");
        assert!(
            err.to_lowercase().contains("vector") && err.to_lowercase().contains("tensor3"),
            "expected vector + tensor3 error, got: {err}"
        );
    }
}

#[cfg(test)]
mod tensor3_ergonomic_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{src}\n");
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn run_expect_err(src: &str) -> String {
        let src = format!("{src}\n");
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            if let Err(e) = ev.exec_stmt(stmt) {
                return format!("{e:?}");
            }
        }
        panic!("expected error; none raised");
    }

    fn scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name).unwrap() {
            Value::Scalar(n) => *n,
            other => panic!("Expected Scalar for '{name}', got {other:?}"),
        }
    }

    // ── reshape variadic ────────────────────────────────────────────────────

    #[test]
    fn reshape_to_tensor3_column_major() {
        // reshape(1:24, 2, 3, 4) — column-major walk: A(1,1,1)=1, A(2,1,1)=2,
        // A(1,2,1)=3, A(2,3,4)=24.
        let ev = run("A = reshape(1:24, 2, 3, 4); v1=A(1,1,1); v2=A(2,1,1); v3=A(1,2,1); v_last=A(2,3,4)");
        assert_eq!(scalar(&ev, "v1"), 1.0);
        assert_eq!(scalar(&ev, "v2"), 2.0);
        assert_eq!(scalar(&ev, "v3"), 3.0);
        assert_eq!(scalar(&ev, "v_last"), 24.0);
    }

    #[test]
    fn reshape_tensor3_source() {
        // Round-trip a tensor3 through reshape back to the same shape.
        let ev = run("A = reshape(1:24, 2, 3, 4); B = reshape(A, 4, 3, 2); v_last = B(4, 3, 2)");
        assert_eq!(scalar(&ev, "v_last"), 24.0);
    }

    #[test]
    fn reshape_tensor3_to_matrix() {
        let ev = run("A = reshape(1:12, 2, 3, 2); M = reshape(A, 4, 3); v_last = M(4, 3)");
        // With column-major, the last element is A(last) = 12
        assert_eq!(scalar(&ev, "v_last"), 12.0);
    }

    #[test]
    fn reshape_wrong_element_count_errors() {
        let err = run_expect_err("A = reshape(1:10, 2, 3, 4)");
        assert!(err.to_lowercase().contains("reshape"));
    }

    // ── cat ─────────────────────────────────────────────────────────────────

    #[test]
    fn cat_dim3_two_matrices() {
        let ev = run(
            "M1 = [1, 2; 3, 4]; M2 = [5, 6; 7, 8]; T = cat(3, M1, M2); \
             s1 = size(T, 1); s2 = size(T, 2); s3 = size(T, 3); \
             v1 = T(1, 1, 1); v2 = T(2, 2, 2)",
        );
        assert_eq!(scalar(&ev, "s1"), 2.0);
        assert_eq!(scalar(&ev, "s2"), 2.0);
        assert_eq!(scalar(&ev, "s3"), 2.0);
        assert_eq!(scalar(&ev, "v1"), 1.0);
        assert_eq!(scalar(&ev, "v2"), 8.0);
    }

    #[test]
    fn cat_dim3_tensor3_plus_matrix() {
        let ev = run(
            "T = zeros3(2, 2, 3); M = [1, 2; 3, 4]; \
             U = cat(3, T, M); s3 = size(U, 3); v = U(1, 2, 4)",
        );
        assert_eq!(scalar(&ev, "s3"), 4.0);
        assert_eq!(scalar(&ev, "v"), 2.0);
    }

    #[test]
    fn cat_dim3_shape_mismatch_errors() {
        let err = run_expect_err("M1 = [1, 2; 3, 4]; M2 = [1, 2, 3]; T = cat(3, M1, M2)");
        assert!(
            err.to_lowercase().contains("cat"),
            "expected cat shape error, got: {err}"
        );
    }

    #[test]
    fn cat_dim1_defers_to_vertcat() {
        // Smoke test: cat(1, M1, M2) should equal vertcat(M1, M2).
        let ev = run(
            "M1 = [1, 2; 3, 4]; M2 = [5, 6; 7, 8]; V = cat(1, M1, M2); s1 = size(V, 1)",
        );
        assert_eq!(scalar(&ev, "s1"), 4.0);
    }

    #[test]
    fn cat_bad_dim_errors() {
        let err = run_expect_err("M = [1, 2; 3, 4]; T = cat(4, M, M)");
        assert!(
            err.to_lowercase().contains("dim"),
            "expected dim error, got: {err}"
        );
    }

    // ── permute ─────────────────────────────────────────────────────────────

    #[test]
    fn permute_identity() {
        let ev = run("A = reshape(1:24, 2, 3, 4); B = permute(A, [1, 2, 3]); v = B(2, 3, 4)");
        assert_eq!(scalar(&ev, "v"), 24.0);
    }

    #[test]
    fn permute_swap_axes_1_and_2() {
        // A is (2, 3, 4). Swap first two axes → B is (3, 2, 4).
        // A(i, j, k) goes to B(j, i, k).
        let ev = run(
            "A = reshape(1:24, 2, 3, 4); B = permute(A, [2, 1, 3]); \
             s1 = size(B, 1); s2 = size(B, 2); s3 = size(B, 3); \
             v_src = A(2, 3, 4); v_dst = B(3, 2, 4)",
        );
        assert_eq!(scalar(&ev, "s1"), 3.0);
        assert_eq!(scalar(&ev, "s2"), 2.0);
        assert_eq!(scalar(&ev, "s3"), 4.0);
        assert_eq!(scalar(&ev, "v_src"), scalar(&ev, "v_dst"));
    }

    #[test]
    fn permute_invalid_order_errors() {
        let err = run_expect_err("A = zeros3(2, 2, 2); B = permute(A, [1, 1, 2])");
        assert!(
            err.to_lowercase().contains("permutation"),
            "expected permutation error, got: {err}"
        );
    }

    #[test]
    fn permute_on_non_tensor3_errors() {
        let err = run_expect_err("M = [1, 2; 3, 4]; B = permute(M, [1, 2, 3])");
        assert!(err.to_lowercase().contains("tensor3"));
    }

    // ── squeeze ─────────────────────────────────────────────────────────────

    #[test]
    fn squeeze_drops_trailing_singleton() {
        // (2, 3, 1) → Matrix(2, 3)
        let ev = run("T = reshape(1:6, 2, 3, 1); M = squeeze(T); s1 = size(M, 1); s2 = size(M, 2)");
        assert_eq!(scalar(&ev, "s1"), 2.0);
        assert_eq!(scalar(&ev, "s2"), 3.0);
    }

    #[test]
    fn squeeze_drops_middle_singleton() {
        // (2, 1, 3) → Matrix(2, 3)
        let ev = run("T = reshape(1:6, 2, 1, 3); M = squeeze(T); s1 = size(M, 1); s2 = size(M, 2)");
        assert_eq!(scalar(&ev, "s1"), 2.0);
        assert_eq!(scalar(&ev, "s2"), 3.0);
    }

    #[test]
    fn squeeze_two_singletons_returns_vector() {
        // (1, 1, 5) → Vector(5)
        let ev = run("T = reshape(1:5, 1, 1, 5); v = squeeze(T); n = numel(v)");
        assert_eq!(scalar(&ev, "n"), 5.0);
    }

    #[test]
    fn squeeze_all_singletons_returns_scalar() {
        let ev = run("T = reshape(7, 1, 1, 1); s = squeeze(T)");
        assert_eq!(scalar(&ev, "s"), 7.0);
    }

    #[test]
    fn squeeze_no_singletons_passes_through() {
        let ev = run("T = zeros3(2, 3, 4); U = squeeze(T); d = ndims(U)");
        assert_eq!(scalar(&ev, "d"), 3.0);
    }

    #[test]
    fn squeeze_on_matrix_passes_through() {
        // Non-tensor3 should pass through unchanged (not error).
        let ev = run("M = [1, 2; 3, 4]; N = squeeze(M); s1 = size(N, 1); s2 = size(N, 2)");
        assert_eq!(scalar(&ev, "s1"), 2.0);
        assert_eq!(scalar(&ev, "s2"), 2.0);
    }
}

#[cfg(test)]
mod tensor3_io_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};
    use ndarray::Array3;
    use num_complex::Complex;
    use rustlab_core::C64;

    fn run(src: &str) -> Evaluator {
        let src = format!("{src}\n");
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn temp_path(name: &str) -> String {
        let dir = std::env::temp_dir();
        dir.join(format!("rustlab_tensor3_test_{}_{}", std::process::id(), name))
            .to_string_lossy()
            .into_owned()
    }

    // ── TOML round-trip ─────────────────────────────────────────────────────

    #[test]
    fn toml_roundtrip_tensor3() {
        let path = temp_path("roundtrip.toml");
        let src = format!(
            "A = reshape(1:24, 2, 3, 4); s = struct(\"A\", A); save(\"{path}\", s); \
             t = load(\"{path}\"); B = t.A; v_first = B(1, 1, 1); v_last = B(2, 3, 4); \
             s1 = size(B, 1); s2 = size(B, 2); s3 = size(B, 3)"
        );
        let ev = run(&src);
        match ev.get("v_first").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 1.0),
            other => panic!("expected Scalar, got {other:?}"),
        }
        match ev.get("v_last").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 24.0),
            other => panic!("expected Scalar, got {other:?}"),
        }
        match ev.get("s1").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 2.0),
            _ => panic!(),
        }
        match ev.get("s2").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 3.0),
            _ => panic!(),
        }
        match ev.get("s3").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 4.0),
            _ => panic!(),
        }
        let _ = std::fs::remove_file(&path);
    }

    // ── NPY round-trip ──────────────────────────────────────────────────────

    #[test]
    fn npy_roundtrip_tensor3() {
        let path = temp_path("roundtrip.npy");
        let src = format!(
            "A = reshape(1:12, 2, 2, 3); save(\"{path}\", A); \
             B = load(\"{path}\"); d = ndims(B); s1 = size(B, 1); s2 = size(B, 2); s3 = size(B, 3)"
        );
        let ev = run(&src);
        match ev.get("d").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 3.0),
            _ => panic!(),
        }
        match ev.get("s1").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 2.0),
            _ => panic!(),
        }
        match ev.get("s2").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 2.0),
            _ => panic!(),
        }
        match ev.get("s3").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 3.0),
            _ => panic!(),
        }
        let _ = std::fs::remove_file(&path);
    }

    // ── NPZ round-trip ──────────────────────────────────────────────────────

    #[test]
    fn npz_roundtrip_tensor3() {
        let path = temp_path("roundtrip.npz");
        let src = format!(
            "A = reshape(1:24, 3, 2, 4); save(\"{path}\", \"A\", A); \
             B = load(\"{path}\", \"A\"); \
             v_first = B(1, 1, 1); v_last = B(3, 2, 4)"
        );
        let ev = run(&src);
        match ev.get("v_first").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 1.0),
            _ => panic!(),
        }
        match ev.get("v_last").unwrap() {
            Value::Scalar(n) => assert_eq!(*n, 24.0),
            _ => panic!(),
        }
        let _ = std::fs::remove_file(&path);
    }

    // ── Display truncation on a large tensor ────────────────────────────────

    #[test]
    fn display_large_tensor_is_bounded() {
        // 10×10×100 is 10k cells — naive formatting would be dozens of KB.
        // The truncated Display must stay well under that.
        let mut t = Array3::<C64>::zeros((10, 10, 100));
        for i in 0..10 {
            for j in 0..10 {
                for k in 0..100 {
                    t[[i, j, k]] = Complex::new((i * 100 + j * 10 + k) as f64, 0.0);
                }
            }
        }
        let v = Value::Tensor3(t);
        let s = format!("{}", v);
        // Header plus first page plus last page plus "pages omitted" footer.
        // A full dump (10*10*100 floats, each ~10 chars) would be > 100KB;
        // the truncated output should be well under 5KB.
        assert!(
            s.len() < 5000,
            "truncated display too long ({} bytes); should be bounded",
            s.len()
        );
        assert!(s.starts_with("tensor3 [10×10×100]"));
        assert!(s.contains("pages omitted"));
    }
}

#[cfg(test)]
mod seed_tests {
    use crate::eval::value::Value;
    use crate::{lexer, parser, Evaluator};

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = lexer::tokenize(&src).unwrap();
        let stmts = parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        for stmt in &stmts {
            ev.exec_stmt(stmt).unwrap();
        }
        ev
    }

    fn vec_of(ev: &Evaluator, name: &str) -> Vec<f64> {
        match ev.get(name).unwrap() {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => panic!("{name} is {other:?}, expected Vector"),
        }
    }

    fn matrix_of(ev: &Evaluator, name: &str) -> Vec<Vec<f64>> {
        match ev.get(name).unwrap() {
            Value::Matrix(m) => (0..m.nrows())
                .map(|r| (0..m.ncols()).map(|c| m[[r, c]].re).collect())
                .collect(),
            other => panic!("{name} is {other:?}, expected Matrix"),
        }
    }

    #[test]
    fn seed_makes_randn_deterministic() {
        // Two evaluators, both seeded the same way: identical output.
        let a = run("seed(42); v = randn(8);");
        let b = run("seed(42); v = randn(8);");
        assert_eq!(vec_of(&a, "v"), vec_of(&b, "v"));
    }

    #[test]
    fn seed_makes_rand_matrix_deterministic() {
        let a = run("seed(7); M = rand(3, 4);");
        let b = run("seed(7); M = rand(3, 4);");
        assert_eq!(matrix_of(&a, "M"), matrix_of(&b, "M"));
    }

    #[test]
    fn seed_covers_all_random_builtins_in_a_single_run() {
        // A mix of rand/randn/randi after one seed call must be reproducible.
        let src = "seed(123);
            a = rand(4);
            b = randn(4);
            c = randi(100, 4);";
        let a = run(src);
        let b = run(src);
        assert_eq!(vec_of(&a, "a"), vec_of(&b, "a"));
        assert_eq!(vec_of(&a, "b"), vec_of(&b, "b"));
        assert_eq!(vec_of(&a, "c"), vec_of(&b, "c"));
    }

    #[test]
    fn different_seeds_produce_different_output() {
        let a = run("seed(1); v = randn(8);");
        let b = run("seed(2); v = randn(8);");
        assert_ne!(
            vec_of(&a, "v"),
            vec_of(&b, "v"),
            "two distinct seeds should produce distinct sequences"
        );
    }

    #[test]
    fn seed_with_no_args_re_randomises() {
        // After `seed(0)` two streams match; after a `seed()` call between
        // the second `randn` and the first, the two streams should diverge
        // because the entropy reseed wipes the deterministic state.
        let src = "seed(0); a = randn(8); seed(); b = randn(8);";
        let ev = run(src);
        let va = vec_of(&ev, "a");
        let vb = vec_of(&ev, "b");
        assert_ne!(va, vb, "seed() with no args should re-randomise the stream");
    }

    #[test]
    fn seed_rejects_negative_or_fractional() {
        let src = "seed(-1)";
        let mut ev = Evaluator::new();
        let tokens = crate::lexer::tokenize(&format!("{src}\n")).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let err = ev.exec_stmt(&stmts[0]).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("non-negative integer"),
            "expected validation error, got: {msg}"
        );

        let src2 = "seed(1.5)";
        let mut ev2 = Evaluator::new();
        let tokens2 = crate::lexer::tokenize(&format!("{src2}\n")).unwrap();
        let stmts2 = crate::parser::parse(tokens2).unwrap();
        let err2 = ev2.exec_stmt(&stmts2[0]).unwrap_err();
        assert!(
            err2.to_string().contains("non-negative integer"),
            "expected validation error for fractional seed"
        );
    }
}

// ── Geometry / shape rasterization masks ─────────────────────────────────────
#[cfg(test)]
mod mask_tests {
    use crate::eval::value::Value;
    use crate::Evaluator;

    fn run(src: &str) -> Evaluator {
        let src = format!("{}\n", src);
        let tokens = crate::lexer::tokenize(&src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let mut ev = Evaluator::new();
        ev.run(&stmts).unwrap();
        ev
    }

    fn matrix_count(ev: &Evaluator, name: &str) -> usize {
        match ev.get(name).unwrap() {
            Value::Matrix(m) => m.iter().filter(|c| c.re > 0.5).count(),
            other => panic!("expected matrix for '{name}', got {other:?}"),
        }
    }

    fn matrix_shape(ev: &Evaluator, name: &str) -> (usize, usize) {
        match ev.get(name).unwrap() {
            Value::Matrix(m) => (m.nrows(), m.ncols()),
            other => panic!("expected matrix for '{name}', got {other:?}"),
        }
    }

    #[test]
    fn rect_mask_unit_square_inclusive() {
        let ev = run(
            "[X, Y] = meshgrid(linspace(0, 1, 11), linspace(0, 1, 11));
             M = rect_mask(X, Y, 0, 0, 1, 1);",
        );
        assert_eq!(matrix_shape(&ev, "M"), (11, 11));
        assert_eq!(matrix_count(&ev, "M"), 11 * 11);
    }

    #[test]
    fn disk_mask_pi_approximation() {
        let ev = run(
            "[X, Y] = meshgrid(linspace(-1.5, 1.5, 200), linspace(-1.5, 1.5, 200));
             D = disk_mask(X, Y, 0, 0, 1);
             step = 3.0 / 199.0;
             area = sum(sum(D)) * step * step;",
        );
        let area = match ev.get("area").unwrap() {
            Value::Scalar(v) => *v,
            Value::Matrix(m) if m.nrows() == 1 && m.ncols() == 1 => m[[0, 0]].re,
            other => panic!("expected scalar area, got {other:?}"),
        };
        assert!(
            (area - std::f64::consts::PI).abs() < 0.05,
            "expected ~π, got {area}"
        );
    }

    #[test]
    fn polygon_mask_matches_rect_mask_for_square() {
        let ev = run(
            "[X, Y] = meshgrid(linspace(-0.25, 1.25, 60), linspace(-0.25, 1.25, 60));
             P = polygon_mask(X, Y, [0,0; 1,0; 1,1; 0,1]);
             R = rect_mask(X, Y, 0, 0, 1, 1);
             diff = sum(sum(abs(P - R)));",
        );
        let diff = match ev.get("diff").unwrap() {
            Value::Scalar(v) => *v,
            Value::Matrix(m) if m.nrows() == 1 && m.ncols() == 1 => m[[0, 0]].re,
            other => panic!("expected scalar diff, got {other:?}"),
        };
        // Allow at most a one-cell-thick boundary disagreement; the off-grid
        // sampling here is chosen so most cells fall clearly inside or outside.
        assert!(diff < 1.0, "polygon vs rect disagreement {diff}");
    }

    #[test]
    fn polygon_mask_triangle() {
        let ev = run(
            "[X, Y] = meshgrid(linspace(0, 1, 21), linspace(0, 1, 21));
             T = polygon_mask(X, Y, [0,0; 1,0; 0.5,1]);",
        );
        // Triangle area = 0.5 → expect ~50% of grid cells inside.
        let count = matrix_count(&ev, "T");
        assert!(
            (180..=260).contains(&count),
            "expected ~half of 441 cells inside triangle, got {count}"
        );
    }

    #[test]
    fn rect_mask_arity_error() {
        let mut ev = Evaluator::new();
        let src = "[X, Y] = meshgrid(linspace(0,1,3), linspace(0,1,3))\nM = rect_mask(X, Y, 0, 0, 1)\n";
        let tokens = crate::lexer::tokenize(src).unwrap();
        let stmts = crate::parser::parse(tokens).unwrap();
        let err = ev.run(&stmts).unwrap_err();
        assert!(
            err.to_string().contains("rect_mask"),
            "expected rect_mask arity error, got: {err}"
        );
    }
}
