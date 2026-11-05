export function Gate() {
  const [open, setOpen] = createSignal(true);
  return (
    <Show when={open()} fallback={<p>Closed</p>}>
      <p>Open</p>
    </Show>
  );
}
