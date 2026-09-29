// A function read straight off the token stream. What this file cannot take apart is not in the
// accepted grammar, and the refusal says which part failed.

use proc_macro::{Delimiter, Group, Ident, Spacing, Span, TokenStream, TokenTree};

pub struct Wrong {
    pub at: Span,
    pub says: String,
}

pub fn wrong<T>(at: Span, says: impl Into<String>) -> Result<T, Wrong> {
    Err(Wrong {
        at,
        says: says.into(),
    })
}

pub fn refuse(w: Wrong) -> TokenStream {
    let error: TokenStream = format!("compile_error!({:?});", w.says)
        .parse()
        .expect("a string literal parses");
    error
        .into_iter()
        .map(|mut t| {
            if let TokenTree::Group(g) = &t {
                let mut g2 = Group::new(g.delimiter(), g.stream());
                g2.set_span(w.at);
                t = TokenTree::Group(g2);
            }
            t.set_span(w.at);
            t
        })
        .collect()
}

pub fn is(t: &TokenTree, c: char) -> bool {
    matches!(t, TokenTree::Punct(p) if p.as_char() == c)
}

pub fn word(t: &TokenTree, w: &str) -> bool {
    matches!(t, TokenTree::Ident(i) if i.to_string() == w)
}

pub fn text(tokens: &[TokenTree]) -> String {
    tokens.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(" ")
}

/// Split on a top-level `,`. Angle brackets are not groups to the tokenizer, so their depth is
/// counted here: `HashMap<K, V>` is one parameter type.
pub fn commas(tokens: &[TokenTree]) -> Vec<Vec<TokenTree>> {
    let mut out = vec![Vec::new()];
    let mut depth = 0i32;
    for (at, t) in tokens.iter().enumerate() {
        let arrow = at > 0 && is(&tokens[at - 1], '-');
        match t {
            TokenTree::Punct(p) if p.as_char() == '<' => depth += 1,
            TokenTree::Punct(p) if p.as_char() == '>' && !arrow => depth -= 1,
            TokenTree::Punct(p) if p.as_char() == ',' && depth == 0 => {
                out.push(Vec::new());
                continue;
            }
            _ => {}
        }
        out.last_mut().expect("never empty").push(t.clone());
    }
    out.retain(|one| !one.is_empty());
    out
}

/// The first `:` that is not half of a `::`.
pub fn colon(tokens: &[TokenTree]) -> Option<usize> {
    (0..tokens.len()).find(|&at| {
        let TokenTree::Punct(p) = &tokens[at] else {
            return false;
        };
        let joined = p.spacing() == Spacing::Joint && tokens.get(at + 1).is_some_and(|t| is(t, ':'));
        let second = at > 0
            && matches!(&tokens[at - 1], TokenTree::Punct(q) if q.as_char() == ':' && q.spacing() == Spacing::Joint);
        p.as_char() == ':' && !joined && !second
    })
}

pub struct Param {
    pub pattern: Vec<TokenTree>,
    pub ty: Vec<TokenTree>,
}

impl Param {
    /// The one identifier the pattern binds, if it is that simple.
    pub fn name(&self) -> Option<&Ident> {
        match self.pattern.as_slice() {
            [TokenTree::Ident(i)] => Some(i),
            [m, TokenTree::Ident(i)] if word(m, "mut") => Some(i),
            _ => None,
        }
    }

    pub fn at(&self) -> Span {
        self.pattern[0].span()
    }

    pub fn written(&self) -> String {
        format!("{}: {}", text(&self.pattern), text(&self.ty))
    }

    /// A `&C` or `&'a C`.
    pub fn shared(&self) -> bool {
        self.ty.first().is_some_and(|t| is(t, '&')) && !self.unique()
    }

    pub fn unique(&self) -> bool {
        self.pointee().is_some()
    }

    /// `T` of a `&mut T` or a `&'a mut T`.
    pub fn pointee(&self) -> Option<Vec<TokenTree>> {
        let rest = self.ty.split_first().filter(|(a, _)| is(a, '&'))?.1;
        let rest = if rest.first().is_some_and(|t| is(t, '\'')) {
            &rest[2..]
        } else {
            rest
        };
        match rest.split_first() {
            Some((m, pointee)) if word(m, "mut") => Some(pointee.to_vec()),
            _ => None,
        }
    }
}

pub struct Fun {
    /// The attributes to keep, which excludes a consumed `#[ordered]`.
    pub attrs: Vec<TokenTree>,
    /// The arguments of `#[ordered(...)]`, and where it was written.
    pub ordered: Option<(Vec<TokenTree>, Span)>,
    pub vis: Vec<TokenTree>,
    pub name: Ident,
    pub receiver: Option<Vec<TokenTree>>,
    pub params: Vec<Param>,
    /// `E` of `-> Result<(), E>`, or `None` for a function that returns nothing and cannot fail.
    pub error: Option<Vec<TokenTree>>,
    pub body: Group,
}

/// The last path segment of an attribute, and its arguments.
fn attribute(g: &Group) -> Option<(String, Vec<TokenTree>)> {
    let inner: Vec<TokenTree> = g.stream().into_iter().collect();
    let (args, path) = match inner.split_last() {
        Some((TokenTree::Group(a), path)) if a.delimiter() == Delimiter::Parenthesis => {
            (a.stream().into_iter().collect(), path)
        }
        _ => (Vec::new(), &inner[..]),
    };
    match path.last() {
        Some(TokenTree::Ident(i)) => Some((i.to_string(), args)),
        _ => None,
    }
}

pub fn function(item: TokenStream) -> Result<Fun, Wrong> {
    let tokens: Vec<TokenTree> = item.into_iter().collect();
    let mut at = 0;
    let mut attrs = Vec::new();
    let mut ordered = None;
    while at + 1 < tokens.len() && is(&tokens[at], '#') {
        let TokenTree::Group(g) = &tokens[at + 1] else {
            return wrong(tokens[at].span(), "an attribute is `#[...]`");
        };
        match attribute(g) {
            Some((name, args)) if name == "ordered" => ordered = Some((args, tokens[at].span())),
            Some((name, _)) if name == "parallel" => {
                return wrong(tokens[at].span(), "a function is `#[parallel]` once");
            }
            _ => attrs.extend_from_slice(&tokens[at..at + 2]),
        }
        at += 2;
    }
    let mut vis = Vec::new();
    if tokens.get(at).is_some_and(|t| word(t, "pub")) {
        vis.push(tokens[at].clone());
        at += 1;
        if let Some(TokenTree::Group(g)) = tokens.get(at)
            && g.delimiter() == Delimiter::Parenthesis
        {
            vis.push(tokens[at].clone());
            at += 1;
        }
    }
    let Some(fn_at) = tokens[at..].iter().position(|t| word(t, "fn")) else {
        return wrong(Span::call_site(), "`#[parallel]` goes on a function");
    };
    if let Some(q) = tokens.get(at).filter(|_| fn_at > 0) {
        return wrong(
            q.span(),
            format!("`#[parallel]` takes a plain `fn`, not `{} fn`: a unit of work is done when it returns", q),
        );
    }
    at += 1;
    let Some(TokenTree::Ident(name)) = tokens.get(at) else {
        return wrong(tokens[at - 1].span(), "a function has a name");
    };
    at += 1;
    let params = match tokens.get(at) {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => g,
        Some(t) => {
            return wrong(
                t.span(),
                "`#[parallel]` takes no generic parameters: `invoke!` names one function",
            );
        }
        None => return wrong(name.span(), "a function has parameters"),
    };
    at += 1;
    let Some((TokenTree::Group(body), signature)) = tokens[at..].split_last() else {
        return wrong(name.span(), "a function has a body");
    };
    let error = match signature {
        [] => None,
        _ => Some(result_error(signature).ok_or_else(|| Wrong {
            at: name.span(),
            says: "`#[parallel]` returns `Result<(), E>` or nothing: `invoke!` returns the first `Err` in list order".to_string(),
        })?),
    };
    let mut receiver = None;
    let mut list = Vec::new();
    for one in commas(&params.stream().into_iter().collect::<Vec<_>>()) {
        match colon(&one) {
            Some(c) if !one[..c].iter().any(|t| word(t, "self")) => list.push(Param {
                pattern: one[..c].to_vec(),
                ty: one[c + 1..].to_vec(),
            }),
            _ => receiver = Some(one),
        }
    }
    Ok(Fun {
        attrs,
        ordered,
        vis,
        name: name.clone(),
        receiver,
        params: list,
        error,
        body: body.clone(),
    })
}

/// `E` of `-> Result<(), E>`, `-> result::Result<(), E>` and the like.
fn result_error(signature: &[TokenTree]) -> Option<Vec<TokenTree>> {
    let [dash, arrow, rest @ ..] = signature else {
        return None;
    };
    if !is(dash, '-') || !is(arrow, '>') {
        return None;
    }
    let open = rest.iter().position(|t| is(t, '<'))?;
    let (last, args) = rest[open + 1..].split_last()?;
    if !is(last, '>') || !rest[..open].last().is_some_and(|t| word(t, "Result")) {
        return None;
    }
    match commas(args).as_slice() {
        [unit, error] if matches!(unit.as_slice(), [TokenTree::Group(g)] if g.delimiter() == Delimiter::Parenthesis && g.stream().is_empty()) => {
            Some(error.clone())
        }
        _ => None,
    }
}

/// The header of an `impl` that names a struct again: its parameters without their defaults, the
/// arguments that name them, and its `where` clause.
pub fn declared(tokens: &[TokenTree]) -> Result<String, Wrong> {
    let Some(at) = tokens.iter().position(|t| word(t, "struct")) else {
        return wrong(Span::call_site(), "`#[process]` goes on a struct");
    };
    let Some(TokenTree::Ident(name)) = tokens.get(at + 1) else {
        return wrong(tokens[at].span(), "a struct has a name");
    };
    let text = |t: &[TokenTree]| t.iter().cloned().collect::<TokenStream>().to_string();
    let mut rest = &tokens[at + 2..];
    let (mut params, mut args) = (Vec::new(), Vec::new());
    if rest.first().is_some_and(|t| is(t, '<')) {
        let mut depth = 0;
        let close = (0..rest.len()).position(|i| {
            depth += is(&rest[i], '<') as i32 - (is(&rest[i], '>') && !is(&rest[i - 1], '-')) as i32;
            depth == 0
        });
        let Some(close) = close else { return wrong(name.span(), "unclosed generics") };
        for one in commas(&rest[1..close]) {
            // A parameter's name is its first token, after `const`, and with a lifetime's `'`.
            let from = usize::from(word(&one[0], "const"));
            args.push(text(&one[from..from + 1 + usize::from(is(&one[from], '\''))]));
            let mut depth = 0;
            let default = one.iter().position(|t| {
                depth += is(t, '<') as i32 - is(t, '>') as i32;
                depth == 0 && is(t, '=')
            });
            params.push(text(&one[..default.unwrap_or(one.len())]));
        }
        rest = &rest[close + 1..];
    }
    let stop = |t: &&TokenTree| matches!(t, TokenTree::Group(g) if g.delimiter() == Delimiter::Brace) || is(t, ';');
    let predicates: Vec<TokenTree> = match rest.iter().position(|t| word(t, "where")) {
        Some(w) => rest[w + 1..].iter().take_while(|t| !stop(t)).cloned().collect(),
        None => Vec::new(),
    };
    Ok(format!(
        "impl<{}> {name}<{}> where {} {{ #[doc(hidden)] pub const fn __declared(&self) {{}} }}",
        params.join(", "),
        args.join(", "),
        text(&predicates)
    ))
}
