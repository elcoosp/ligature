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
  const [tag, setTag] = createSignal("in");
  return (
    <div>
      <Panel
        title="Status"
        header={
          <div>
            <Show when={on()}>
              <Slot name="inner"><em class="none">{tag}</em></Slot>
            </Show>
          </div>
        }
        inner={<b class="hl">{tag}</b>}
      />
      <Panel
        title="Bare"
        header={
          <div>
            <Show when={on()}>
              <Slot name="inner"><em class="none">{tag}</em></Slot>
            </Show>
          </div>
        }
      />
    </div>
  );
}
