use std::collections::HashMap;

/// Reasons the recorded compile commands may be incomplete.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Warnings(HashMap<String, usize>);

impl Warnings {
  pub fn new() -> Warnings {
    Warnings::default()
  }

  /// Adds `message`, unless it is already here.
  pub fn push(&mut self, message: impl Into<String>) {
    let next = self.0.len();
    self.0.entry(message.into()).or_insert(next);
  }

  #[cfg(test)]
  pub fn len(&self) -> usize {
    self.0.len()
  }

  #[cfg(test)]
  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  /// The messages in the order they were first added.
  pub fn in_order(&self) -> Vec<&str> {
    let mut ordered = vec![""; self.0.len()];
    for (message, &i) in &self.0 {
      ordered[i] = message;
    }
    ordered
  }
}

/// The messages in the order they were first added.
impl IntoIterator for Warnings {
  type Item = String;
  type IntoIter = std::vec::IntoIter<String>;

  fn into_iter(self) -> Self::IntoIter {
    let mut ordered = vec![String::new(); self.0.len()];
    for (message, i) in self.0 {
      ordered[i] = message;
    }
    ordered.into_iter()
  }
}

/// Adds each message that is not already here, in order.
impl Extend<String> for Warnings {
  fn extend<I: IntoIterator<Item = String>>(&mut self, messages: I) {
    for message in messages {
      self.push(message);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn of(messages: &[&str]) -> Warnings {
    let mut w = Warnings::new();
    for m in messages {
      w.push(*m);
    }
    w
  }

  #[test]
  fn new_is_empty() {
    let w = Warnings::new();
    assert!(w.is_empty());
    assert_eq!(w.len(), 0);
    assert!(w.in_order().is_empty());
  }

  #[test]
  fn len_counts_distinct_messages() {
    assert_eq!(of(&["a", "b", "a"]).len(), 2);
  }

  #[test]
  fn push_keeps_order() {
    assert_eq!(of(&["b", "a"]).in_order(), ["b", "a"]);
  }

  #[test]
  fn push_skips_duplicates() {
    assert_eq!(of(&["a", "b", "a"]).in_order(), ["a", "b"]);
  }

  #[test]
  fn into_iter_is_in_order() {
    let all: Vec<String> = of(&["c", "a", "b"]).into_iter().collect();
    assert_eq!(all, ["c", "a", "b"]);
  }

  #[test]
  fn extends_from_any_strings() {
    let mut w = of(&["a"]);
    w.extend(["b".to_string(), "a".to_string()]);
    assert_eq!(w.in_order(), ["a", "b"]);
  }

  #[test]
  fn extend_appends_and_skips_duplicates() {
    let mut w = of(&["a", "b"]);
    w.extend(of(&["b", "c"]));
    assert_eq!(w.in_order(), ["a", "b", "c"]);
  }
}
