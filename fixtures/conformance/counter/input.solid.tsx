export function Counter() {
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
