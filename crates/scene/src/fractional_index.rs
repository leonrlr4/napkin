//! Excalidraw's fractional indexing.
//!
//! Key generation is ported from `@excalidraw/fractional-indexing` 3.3.0
//! (`packages/fractional-indexing/src/index.ts`), with `digits` fixed to `BASE_62_DIGITS`
//! rather than kept as a parameter. Index synchronization is ported from
//! `packages/element/src/fractionalIndex.ts`'s `syncMovedIndices`, `syncInvalidIndices`,
//! `getMovedIndicesGroups`, `getInvalidIndicesGroups`, `isValidFractionalIndex` and
//! `generateIndices`, plus the parts of `validateFractionalIndices` that apply when called
//! with `shouldThrow: true, includeBoundTextValidation: false, ignoreLogs: true` (as
//! `syncMovedIndices` calls it). `orderByFractionalIndex` and `syncInvalidIndicesImmutable`
//! are not ported: nothing in napkin needs them.

use std::cmp::Ordering;
use std::collections::HashSet;

use rough::js::math_round;

use crate::element::Element;
use crate::env::Env;
use crate::new_element::bump_version;

const DIGITS: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// `digits[0]`.
const ZERO: char = '0';

/// A JS `Error` thrown by the fractional-indexing algorithms; `Display` prints the same
/// message the pinned source's `throw new Error(...)` carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexError(pub String);

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IndexError {}

fn err(message: impl Into<String>) -> IndexError {
    IndexError(message.into())
}

/// JS `a < b` / `a >= b` etc. on strings: UTF-16 code unit order, not Rust's UTF-8 byte
/// order (see the plan's JS -> Rust table).
fn utf16_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// JS truthiness of a string: `undefined`, `null` and `""` are falsy.
fn truthy_str(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

/// `` `A${digits[0].repeat(26)}` `` : the reserved key that has no valid value below it.
fn all_zero_integer() -> String {
    format!("A{}", ZERO.to_string().repeat(26))
}

// --- packages/fractional-indexing/src/index.ts ---

/// `midpoint`. `a` may be empty; `b` is `None` or non-empty (checked by callers).
fn midpoint(a: &str, b: Option<&str>) -> Result<String, IndexError> {
    if let Some(b) = b
        && utf16_cmp(a, b) != Ordering::Less
    {
        return Err(err(format!("{a} >= {b}")));
    }
    let a_chars: Vec<char> = a.chars().collect();
    let b_nonempty = b.filter(|b| !b.is_empty());
    if a_chars.last() == Some(&ZERO) || b_nonempty.is_some_and(|b| b.ends_with(ZERO)) {
        return Err(err("trailing zero"));
    }
    if let Some(b) = b_nonempty {
        let b_chars: Vec<char> = b.chars().collect();
        let mut n = 0usize;
        while b_chars.get(n).copied() == Some(a_chars.get(n).copied().unwrap_or(ZERO)) {
            n += 1;
        }
        if n > 0 {
            let prefix: String = b_chars[..n].iter().collect();
            let a_rest: String = a_chars.get(n..).unwrap_or(&[]).iter().collect();
            let b_rest: String = b_chars[n..].iter().collect();
            return Ok(prefix + &midpoint(&a_rest, Some(&b_rest))?);
        }
    }
    let digit_a: i64 = if a.is_empty() {
        0
    } else {
        index_of_digit(a_chars[0])
    };
    let digit_b: i64 = match b {
        Some(b) => match b.chars().next() {
            Some(c) => index_of_digit(c),
            None => -1,
        },
        None => DIGITS.chars().count() as i64,
    };
    if digit_b - digit_a > 1 {
        let mid_digit = math_round(0.5 * (digit_a as f64 + digit_b as f64)) as i64;
        return Ok(nth_digit(mid_digit).to_string());
    }
    if let Some(b) = b_nonempty
        && b.chars().count() > 1
    {
        return Ok(b.chars().next().expect("non-empty").to_string());
    }
    let rest = midpoint(
        a_chars
            .get(1..)
            .unwrap_or(&[])
            .iter()
            .collect::<String>()
            .as_str(),
        None,
    )?;
    Ok(format!("{}{rest}", nth_digit(digit_a)))
}

fn index_of_digit(c: char) -> i64 {
    DIGITS
        .chars()
        .position(|d| d == c)
        .map(|i| i as i64)
        .unwrap_or(-1)
}

fn nth_digit(i: i64) -> char {
    DIGITS
        .chars()
        .nth(usize::try_from(i).unwrap_or(usize::MAX))
        .expect("digit index in range")
}

fn validate_integer(int: &str) -> Result<(), IndexError> {
    let expected = get_integer_length(int.chars().next())?;
    if int.chars().count() != expected {
        return Err(err(format!("invalid integer part of order key: {int}")));
    }
    Ok(())
}

fn get_integer_length(head: Option<char>) -> Result<usize, IndexError> {
    match head {
        Some(h) if h.is_ascii_lowercase() => Ok(h as usize - 'a' as usize + 2),
        Some(h) if h.is_ascii_uppercase() => Ok('Z' as usize - h as usize + 2),
        _ => Err(err(format!(
            "invalid order key head: {}",
            head.map(String::from)
                .unwrap_or_else(|| "undefined".to_owned())
        ))),
    }
}

fn get_integer_part(key: &str) -> Result<String, IndexError> {
    let integer_part_length = get_integer_length(key.chars().next())?;
    let chars: Vec<char> = key.chars().collect();
    if integer_part_length > chars.len() {
        return Err(err(format!("invalid order key: {key}")));
    }
    Ok(chars[..integer_part_length].iter().collect())
}

/// `validateOrderKey`.
pub fn validate_order_key(key: &str) -> Result<(), IndexError> {
    let valid_chars = key.chars().all(|c| DIGITS.contains(c));
    if key == all_zero_integer() || !valid_chars {
        return Err(err(format!("invalid order key: {key}")));
    }
    // `key` is ASCII-only past this point (every char matched `DIGITS`), so byte slicing
    // by `i.len()` below lands on a char boundary.
    let i = get_integer_part(key)?;
    let f = &key[i.len()..];
    if f.ends_with(ZERO) {
        return Err(err(format!("invalid order key: {key}")));
    }
    Ok(())
}

/// Returns `None` when there is no larger integer part (JS returns `null`).
fn increment_integer(x: &str) -> Result<Option<String>, IndexError> {
    validate_integer(x)?;
    let chars: Vec<char> = x.chars().collect();
    let head = chars[0];
    let mut digs: Vec<char> = chars[1..].to_vec();
    let digit_chars: Vec<char> = DIGITS.chars().collect();
    let mut carry = true;
    let mut i = digs.len();
    while carry && i > 0 {
        i -= 1;
        let d = digit_chars
            .iter()
            .position(|&c| c == digs[i])
            .map_or(0, |p| p + 1);
        if d == digit_chars.len() {
            digs[i] = digit_chars[0];
        } else {
            digs[i] = digit_chars[d];
            carry = false;
        }
    }
    if carry {
        if head == 'Z' {
            return Ok(Some(format!("a{ZERO}")));
        }
        if head == 'z' {
            return Ok(None);
        }
        let h = char::from_u32(head as u32 + 1).expect("char below 'Z' or 'z'");
        if h > 'a' {
            digs.push(digit_chars[0]);
        } else {
            digs.pop();
        }
        return Ok(Some(format!("{h}{}", digs.iter().collect::<String>())));
    }
    Ok(Some(format!("{head}{}", digs.iter().collect::<String>())))
}

/// Returns `None` when there is no smaller integer part (JS returns `null`).
fn decrement_integer(x: &str) -> Result<Option<String>, IndexError> {
    validate_integer(x)?;
    let chars: Vec<char> = x.chars().collect();
    let head = chars[0];
    let mut digs: Vec<char> = chars[1..].to_vec();
    let digit_chars: Vec<char> = DIGITS.chars().collect();
    let last_digit = *digit_chars.last().expect("digits non-empty");
    let mut borrow = true;
    let mut i = digs.len();
    while borrow && i > 0 {
        i -= 1;
        let pos = digit_chars.iter().position(|&c| c == digs[i]);
        match pos {
            Some(0) | None => digs[i] = last_digit,
            Some(p) => {
                digs[i] = digit_chars[p - 1];
                borrow = false;
            }
        }
    }
    if borrow {
        if head == 'a' {
            return Ok(Some(format!("Z{last_digit}")));
        }
        if head == 'A' {
            return Ok(None);
        }
        let h = char::from_u32(head as u32 - 1).expect("char above 'A' or 'a'");
        if h < 'Z' {
            digs.push(last_digit);
        } else {
            digs.pop();
        }
        return Ok(Some(format!("{h}{}", digs.iter().collect::<String>())));
    }
    Ok(Some(format!("{head}{}", digs.iter().collect::<String>())))
}

/// `generateKeyBetween(a, b)`.
pub fn generate_key_between(a: Option<&str>, b: Option<&str>) -> Result<String, IndexError> {
    if let Some(a) = a {
        validate_order_key(a)?;
    }
    if let Some(b) = b {
        validate_order_key(b)?;
    }
    if let (Some(a), Some(b)) = (a, b)
        && utf16_cmp(a, b) != Ordering::Less
    {
        return Err(err(format!("{a} >= {b}")));
    }

    match (a, b) {
        (None, None) => Ok(format!("a{ZERO}")),
        (None, Some(b)) => {
            let ib = get_integer_part(b)?;
            let fb = &b[ib.len()..];
            if ib == all_zero_integer() {
                return Ok(format!("{ib}{}", midpoint("", Some(fb))?));
            }
            if utf16_cmp(&ib, b) == Ordering::Less {
                return Ok(ib);
            }
            decrement_integer(&ib)?.ok_or_else(|| err("cannot decrement any more"))
        }
        (Some(a), None) => {
            let ia = get_integer_part(a)?;
            let fa = &a[ia.len()..];
            match increment_integer(&ia)? {
                Some(i) => Ok(i),
                None => Ok(format!("{ia}{}", midpoint(fa, None)?)),
            }
        }
        (Some(a), Some(b)) => {
            let ia = get_integer_part(a)?;
            let fa = &a[ia.len()..];
            let ib = get_integer_part(b)?;
            let fb = &b[ib.len()..];
            if ia == ib {
                return Ok(format!("{ia}{}", midpoint(fa, Some(fb))?));
            }
            let i = increment_integer(&ia)?.ok_or_else(|| err("cannot increment any more"))?;
            if utf16_cmp(&i, b) == Ordering::Less {
                Ok(i)
            } else {
                Ok(format!("{ia}{}", midpoint(fa, None)?))
            }
        }
    }
}

/// `generateNKeysBetween(a, b, n)`.
pub fn generate_n_keys_between(
    a: Option<&str>,
    b: Option<&str>,
    n: usize,
) -> Result<Vec<String>, IndexError> {
    if n == 0 {
        return Ok(Vec::new());
    }
    if n == 1 {
        return Ok(vec![generate_key_between(a, b)?]);
    }
    if b.is_none() {
        let mut c = generate_key_between(a, b)?;
        let mut result = vec![c.clone()];
        for _ in 0..n - 1 {
            c = generate_key_between(Some(&c), b)?;
            result.push(c.clone());
        }
        return Ok(result);
    }
    if a.is_none() {
        let mut c = generate_key_between(a, b)?;
        let mut result = vec![c.clone()];
        for _ in 0..n - 1 {
            c = generate_key_between(a, Some(&c))?;
            result.push(c.clone());
        }
        result.reverse();
        return Ok(result);
    }
    let mid = n / 2;
    let c = generate_key_between(a, b)?;
    let mut result = generate_n_keys_between(a, Some(&c), mid)?;
    result.push(c.clone());
    result.extend(generate_n_keys_between(Some(&c), b, n - mid - 1)?);
    Ok(result)
}

// --- packages/element/src/fractionalIndex.ts ---

/// `elements[i]?.index` for a possibly out-of-range signed position (JS `undefined`
/// indexing never panics; `-1` and `elements.length` show up as sentinels below).
fn element_index_at(elements: &[Element], i: isize) -> Option<&str> {
    usize::try_from(i)
        .ok()
        .and_then(|i| elements.get(i))
        .and_then(Element::index)
}

fn element_id_at(elements: &[Element], i: isize) -> Option<&str> {
    usize::try_from(i)
        .ok()
        .and_then(|i| elements.get(i))
        .and_then(Element::id)
}

/// `isValidFractionalIndex`.
fn is_valid_fractional_index(
    index: Option<&str>,
    predecessor: Option<&str>,
    successor: Option<&str>,
) -> bool {
    let Some(index) = truthy_str(index) else {
        return false;
    };
    if validate_order_key(index).is_err() {
        return false;
    }
    match (truthy_str(predecessor), truthy_str(successor)) {
        (Some(p), Some(s)) => {
            utf16_cmp(p, index) == Ordering::Less && utf16_cmp(index, s) == Ordering::Less
        }
        (None, Some(s)) => utf16_cmp(index, s) == Ordering::Less,
        (Some(p), None) => utf16_cmp(p, index) == Ordering::Less,
        (None, None) => true,
    }
}

/// `getMovedIndicesGroups`. Each group is `[lowerBoundIndex, ...moved, upperBoundIndex]`,
/// where the bound positions may be `-1` or `elements.len()`.
fn get_moved_indices_groups(elements: &[Element], moved: &HashSet<String>) -> Vec<Vec<isize>> {
    let len = elements.len() as isize;
    let is_moved = |i: isize| element_id_at(elements, i).is_some_and(|id| moved.contains(id));

    let mut groups = Vec::new();
    let mut i: isize = 0;
    while i < len {
        if is_moved(i) {
            let mut group = vec![i - 1, i];
            loop {
                i += 1;
                if i >= len || !is_moved(i) {
                    break;
                }
                group.push(i);
            }
            group.push(i);
            groups.push(group);
        } else {
            i += 1;
        }
    }
    groups
}

/// `getLowerBound`, factored out of `getInvalidIndicesGroups`'s closure: a pure function of
/// the current cached bound plus the position being checked, returning the (possibly
/// updated) bound and its position, exactly as the JS closure's destructuring assignment
/// does at each call site.
fn get_lower_bound(
    elements: &[Element],
    lower_bound_index: isize,
    index: isize,
) -> (Option<&str>, isize) {
    let lower_bound = element_index_at(elements, lower_bound_index);
    let candidate = element_index_at(elements, index - 1);
    let take_candidate = match (truthy_str(lower_bound), truthy_str(candidate)) {
        (None, Some(_)) => true,
        (Some(lb), Some(c)) => utf16_cmp(c, lb) == Ordering::Greater,
        _ => false,
    };
    if take_candidate {
        (candidate, index - 1)
    } else {
        (lower_bound, lower_bound_index)
    }
}

/// `getUpperBound`.
fn get_upper_bound(
    elements: &[Element],
    upper_bound_index: isize,
    index: isize,
) -> (Option<&str>, isize) {
    let upper_bound = element_index_at(elements, upper_bound_index);
    let upper_bound_truthy = truthy_str(upper_bound);
    if upper_bound_truthy.is_some() && index < upper_bound_index {
        return (upper_bound, upper_bound_index);
    }
    let len = elements.len() as isize;
    let mut i = upper_bound_index;
    loop {
        i += 1;
        if i >= len {
            break;
        }
        let candidate = element_index_at(elements, i);
        let take_candidate = match (upper_bound_truthy, truthy_str(candidate)) {
            (None, Some(_)) => true,
            (Some(ub), Some(c)) => utf16_cmp(c, ub) == Ordering::Greater,
            _ => false,
        };
        if take_candidate {
            return (candidate, i);
        }
    }
    (None, i)
}

/// `getInvalidIndicesGroups`. Groups are shaped like [`get_moved_indices_groups`]'s.
fn get_invalid_indices_groups(elements: &[Element]) -> Vec<Vec<isize>> {
    let len = elements.len() as isize;
    let mut groups = Vec::new();

    let mut lower_bound_index: isize = -1;
    let mut upper_bound_index: isize = 0;

    let mut i: isize = 0;
    while i < len {
        let current = element_index_at(elements, i);
        let (lower_bound, lbi) = get_lower_bound(elements, lower_bound_index, i);
        let (upper_bound, ubi) = get_upper_bound(elements, upper_bound_index, i);
        lower_bound_index = lbi;
        upper_bound_index = ubi;

        if is_valid_fractional_index(current, lower_bound, upper_bound) {
            i += 1;
            continue;
        }

        let mut group = vec![lower_bound_index, i];
        loop {
            i += 1;
            if i >= len {
                break;
            }
            let current = element_index_at(elements, i);
            let (next_lower_bound, next_lbi) = get_lower_bound(elements, lower_bound_index, i);
            let (next_upper_bound, next_ubi) = get_upper_bound(elements, upper_bound_index, i);
            if is_valid_fractional_index(current, next_lower_bound, next_upper_bound) {
                break;
            }
            lower_bound_index = next_lbi;
            upper_bound_index = next_ubi;
            group.push(i);
        }
        group.push(upper_bound_index);
        groups.push(group);
    }
    groups
}

/// `generateIndices`: `(position in elements, new index)` pairs, in the same order the
/// JS `Map`'s insertion order would visit them.
fn generate_indices(
    elements: &[Element],
    indices_groups: Vec<Vec<isize>>,
) -> Result<Vec<(usize, String)>, IndexError> {
    let mut updates = Vec::new();
    for mut group in indices_groups {
        let lower_bound_index = group.remove(0);
        let upper_bound_index = group.pop().expect("group has an upper bound");
        let lower = element_index_at(elements, lower_bound_index);
        let upper = element_index_at(elements, upper_bound_index);
        let keys = generate_n_keys_between(lower, upper, group.len())?;
        for (position, key) in group.into_iter().zip(keys) {
            updates.push((position as usize, key));
        }
    }
    Ok(updates)
}

/// The part of `validateFractionalIndices` that runs when called with
/// `includeBoundTextValidation: false`: every element's index must be valid given its
/// neighbors. The bound-text branch is not ported (napkin never sets that flag).
fn all_indices_valid(indices: &[Option<&str>]) -> bool {
    indices.iter().enumerate().all(|(i, &index)| {
        let predecessor = if i == 0 { None } else { indices[i - 1] };
        let successor = indices.get(i + 1).copied().flatten();
        is_valid_fractional_index(index, predecessor, successor)
    })
}

fn apply_updates(elements: &mut [Element], updates: Vec<(usize, String)>, env: &mut impl Env) {
    for (position, new_index) in updates {
        if elements[position].index() != Some(new_index.as_str()) {
            elements[position].set_index(new_index);
            bump_version(&mut elements[position], env);
        }
    }
}

/// The `try` block of `syncMovedIndices`: computes updates and validates them, without
/// mutating anything, so a failure can fall back to `syncInvalidIndices` on unmodified
/// elements.
fn try_sync_moved_indices(
    elements: &[Element],
    moved: &HashSet<String>,
) -> Result<Vec<(usize, String)>, IndexError> {
    let groups = get_moved_indices_groups(elements, moved);
    let updates = generate_indices(elements, groups)?;

    let mut candidates: Vec<Option<&str>> = elements.iter().map(Element::index).collect();
    for (position, key) in &updates {
        candidates[*position] = Some(key.as_str());
    }
    if !all_indices_valid(&candidates) {
        return Err(err("Fractional indices invariant has been compromised"));
    }
    Ok(updates)
}

/// `syncMovedIndices`. `moved` holds the ids of elements whose position in `elements`
/// changed since their `index` was last assigned.
pub fn sync_moved_indices(elements: &mut [Element], moved: &HashSet<String>, env: &mut impl Env) {
    match try_sync_moved_indices(elements, moved) {
        Ok(updates) => apply_updates(elements, updates, env),
        Err(_) => sync_invalid_indices(elements, env),
    }
}

/// `syncInvalidIndices`. Unlike `syncMovedIndices`, the source has no `try`/`catch` here:
/// a `generateIndices` failure is a bug reachable only from already-invalid data, and JS
/// lets it propagate as an uncaught exception, so this panics rather than returning
/// `Result` (napkin's `sync_invalid_indices` has no error case in its public signature).
pub fn sync_invalid_indices(elements: &mut [Element], env: &mut impl Env) {
    let groups = get_invalid_indices_groups(elements);
    let updates =
        generate_indices(elements, groups).expect("indices groups from valid element data");
    apply_updates(elements, updates, env);
}
