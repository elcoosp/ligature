export function Badge({ count }) {
  return <b class="badge">{count}</b>;
}

export function Panel({ title, children }) {
  return (
    <section class="panel">
      <h2>{title}</h2>
      <Slot name="header" />
      <Slot />
    </section>
  );
}

export function App() {
  const [n, setN] = createSignal("two");
  return (
    <Panel title="Status" header={<Badge count={n()} />}>
      <p>grandchild mounted into the fill</p>
    </Panel>
  );
}
