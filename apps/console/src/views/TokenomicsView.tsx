import './tokenomics.css'

/** Tokenomics reports remain available without an operational realm connection. */
export function TokenomicsView() {
  return (
    <iframe
      className="tokenomics-frame"
      title="Tokenomics dashboard"
      src="/tokenomics/index.html?theme=light"
    />
  )
}
