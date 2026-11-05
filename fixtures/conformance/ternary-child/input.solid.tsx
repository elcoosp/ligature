export function Flip() {
  const [on, setOn] = createSignal(false);
  return (
    <div>
      {on() ? <b>yes</b> : <i>no</i>}
    </div>
  );
}
