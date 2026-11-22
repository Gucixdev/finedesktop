//! The menu calculator: + - * / % ^ (also `x` for times), parentheses, unary minus, `,` as decimal point,
//! pi, e and sqrt sin cos tan ln log exp abs floor ceil round. `-2^2` is -4, `^` is right associative.

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

type R = Result<f64, ()>;

impl P<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> R {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v += self.term()?;
            } else if self.eat(b'-') {
                v -= self.term()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn term(&mut self) -> R {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') || self.eat(b'x') {
                v *= self.unary()?;
            } else if self.eat(b'/') {
                v /= self.unary()?;
            } else if self.eat(b'%') {
                v %= self.unary()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn unary(&mut self) -> R {
        if self.eat(b'-') {
            return Ok(-self.unary()?);
        }
        if self.eat(b'+') {
            return self.unary();
        }
        let b = self.primary()?;
        if self.eat(b'^') {
            return Ok(b.powf(self.unary()?)); // right associative, and binds tighter than the minus in front
        }
        Ok(b)
    }

    fn close(&mut self) -> Result<(), ()> {
        if self.eat(b')') { Ok(()) } else { Err(()) }
    }

    fn primary(&mut self) -> R {
        self.ws();
        if self.eat(b'(') {
            let v = self.expr()?;
            self.close()?;
            return Ok(v);
        }
        let c = *self.s.get(self.i).ok_or(())?;
        if c.is_ascii_digit() || c == b'.' || c == b',' {
            let st = self.i;
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == b'.' || *c == b',') {
                self.i += 1;
            }
            return std::str::from_utf8(&self.s[st..self.i]).map_err(|_| ())?.replace(',', ".").parse().map_err(|_| ());
        }
        if c.is_ascii_alphabetic() {
            let st = self.i;
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphabetic()) {
                self.i += 1;
            }
            let name = std::str::from_utf8(&self.s[st..self.i]).map_err(|_| ())?.to_lowercase();
            match name.as_str() {
                "pi" => return Ok(std::f64::consts::PI),
                "e" => return Ok(std::f64::consts::E),
                _ => {}
            }
            if !self.eat(b'(') {
                return Err(());
            }
            let v = self.expr()?;
            self.close()?;
            return Ok(match name.as_str() {
                "sqrt" => v.sqrt(),
                "sin" => v.sin(),
                "cos" => v.cos(),
                "tan" => v.tan(),
                "ln" => v.ln(),
                "log" => v.log10(),
                "exp" => v.exp(),
                "abs" => v.abs(),
                "floor" => v.floor(),
                "ceil" => v.ceil(),
                "round" => v.round(),
                _ => return Err(()),
            });
        }
        Err(())
    }
}

/// Value of `text`, or None on a syntax error.
pub fn eval(text: &str) -> Option<f64> {
    let mut p = P { s: text.as_bytes(), i: 0 };
    let v = p.expr().ok()?;
    p.ws();
    (p.i == p.s.len()).then_some(v)
}

/// "14", "0.5", "3.14159265359": up to 12 significant digits, no trailing zeros.
pub fn show(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{:.*}", (11 - v.abs().log10().floor() as i32).clamp(0, 300) as usize, v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(eval("2+3*4"), Some(14.0));
        assert_eq!(eval("(2+3)*4"), Some(20.0));
        assert_eq!(eval("2 x 3"), Some(6.0));
        assert_eq!(eval("10 % 4"), Some(2.0));
        assert_eq!(eval("2^3^2"), Some(512.0));
        assert_eq!(eval("-2^2"), Some(-4.0));
        assert_eq!(eval("1,5+1"), Some(2.5));
    }

    #[test]
    fn functions_and_constants() {
        assert_eq!(eval("sqrt(16)+abs(-1)"), Some(5.0));
        assert!((eval("2*pi").unwrap() - 6.283185307).abs() < 1e-6);
        assert_eq!(eval("round(2.6)"), Some(3.0));
    }

    #[test]
    fn errors_are_none() {
        for bad in ["", "2+", "(1", "1)", "foo(1)", "2 3", "sin"] {
            assert_eq!(eval(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn results_print_short() {
        assert_eq!(show(14.0), "14");
        assert_eq!(show(0.5), "0.5");
        assert_eq!(show(1.0 / 3.0), "0.333333333333");
        assert_eq!(show(-2.5), "-2.5");
    }
}
