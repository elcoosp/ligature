//! Shared test/bench fixtures. Public so benches and the fixture harness
//! reuse exactly the contract-test sources.

/// The S1 `counter` fixture (§9.2): signal + memo + Show branch + static class.
pub const COUNTER_TSX: &str = r#"export function Counter() {
  const [count, setCount] = createSignal(0);
  const doubled = createMemo(() => count() * 2);
  return (
    <div class="counter p-4" style={{ display: "flex", padding: "16px" }}>
      <button onClick={() => setCount(count() + 1)}>Add</button>
      <span>{count()}</span>
      <Show when={doubled() > 10}>
        <p class="hot">Big: {doubled()}</p>
      </Show>
    </div>
  );
}
"#;

/// A `.map` list fixture (unkeyed For).
pub const ITEMS_TSX: &str = r"export function Items() {
  const [items, setItems] = createSignal(0);
  return (
    <ul>
      {items.map((item) => <li>{item}</li>)}
    </ul>
  );
}
";
