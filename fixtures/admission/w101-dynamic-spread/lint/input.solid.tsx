export function Chip({ label }) {
  const [title, setTitle] = createSignal("tip");
  return <div class="chip" {...{ title: title() }}>{label}</div>;
}
