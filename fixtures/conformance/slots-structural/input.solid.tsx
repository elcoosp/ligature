export function Panel({ title }) {
  return (
    <section class="panel">
      <h2>{title}</h2>
      <Slot name="header" />
      <Slot name="foot">
        <div>
          <Show when={title === "Status"}>
            <i class="fb">{title}</i>
          </Show>
        </div>
      </Slot>
    </section>
  );
}

export function App() {
  const [on, setOn] = createSignal(true);
  const [label, setLabel] = createSignal("live");
  const [rows, setRows] = createSignal(["alpha", "beta"]);
  return (
    <Panel
      title="Status"
      header={
        <div>
          <Show when={on()}>
            <b class="hl">{label}</b>
          </Show>
          <ul>
            <For each={rows()} key={(t) => t}>{(t) => <li>{t}</li>}</For>
          </ul>
        </div>
      }
    />
  );
}
