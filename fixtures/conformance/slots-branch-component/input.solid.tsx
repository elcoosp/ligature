export function Badge({ count }) {
  return <b class="badge">{count}</b>;
}

export function Panel({ title }) {
  return (
    <section class="panel">
      <h2>{title}</h2>
      <Slot name="header" />
    </section>
  );
}

export function App() {
  const [on, setOn] = createSignal(true);
  const [n, setN] = createSignal("two");
  return (
    <Panel
      title="Status"
      header={
        <div>
          <Show when={on()}>
            <Badge count={n()} />
          </Show>
        </div>
      }
    />
  );
}
