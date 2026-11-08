export function App() {
  const [on, setOn] = createSignal(true);
  const [tag, setTag] = createSignal("in");
  return (
    <div>
      <Show when={on()} fallback={<i class="off">{tag}</i>}>
        <b class="on">{tag}</b>
      </Show>
      <Show when={on()} fallback={<Badge note="off" />}>
        <span>live</span>
      </Show>
    </div>
  );
}

export function Badge({ note }) {
  return <em class="badge">{note}</em>;
}
