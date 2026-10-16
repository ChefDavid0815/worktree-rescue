import { LogoMark } from "../components/LogoMark";

export function BootScreen() {
  return (
    <div className="boot" aria-busy="true">
      <LogoMark size={44} />
    </div>
  );
}
