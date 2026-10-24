//! Numbers and colors as the two writers of xochitl's PDFs format them: PDFium (the annotation
//! stream) and Qt's PDF engine (the page stream).

/// Number as PDFium writes it: no trailing zeros, no leading `0` before the point.
pub fn num(v: f32) -> String {
    let mut s = format!("{:.6}", v);
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if s == "-0" {
        s = "0".into();
    }
    if let Some(r) = s.strip_prefix("0.") {
        s = format!(".{r}");
    } else if let Some(r) = s.strip_prefix("-0.") {
        s = format!("-.{r}");
    }
    s
}

/// Number as Qt's PDF engine writes it (`qt_real_to_string`): nine significant digits, the
/// fraction truncated, trailing zeros kept.
pub fn qnum(v: f64) -> String {
    let mut s = String::new();
    let mut v = v;
    if v < 0.0 {
        s.push('-');
        v = -v;
    }
    let mut ival = v as u32;
    let mut ifrac = ((v - ival as f64) * 1e9) as i32;
    if ifrac == 1_000_000_000 {
        ival += 1;
        ifrac = 0;
    }
    let mut fact = 100_000_000;
    if ival == 0 {
        s.push('0');
    } else {
        let digits = ival.to_string();
        for _ in 0..digits.len() {
            fact /= 10;
            ifrac /= 10;
        }
        s += &digits;
    }
    if ifrac != 0 {
        s.push('.');
        while fact != 0 {
            s.push((b'0' + ((ifrac / fact) % 10) as u8) as char);
            fact /= 10;
        }
    }
    s
}

/// A color channel as Qt hands it to the PDF engine (a float).
fn chan(c: u32, shift: u32) -> String {
    qnum((((c >> shift) & 0xff) as f32 / 255.0) as f64)
}

pub fn qrgb(c: u32) -> String {
    format!("{} {} {}", chan(c, 16), chan(c, 8), chan(c, 0))
}

pub fn rgb(c: [f32; 3]) -> String {
    format!("{} {} {}", num(c[0]), num(c[1]), num(c[2]))
}
