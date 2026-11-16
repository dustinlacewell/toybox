/** One SVG path keeps dense waveforms cheap to display in a grid. */
export function waveformPath(peaks:readonly number[]) {
  const step=1000/Math.max(1,peaks.length);
  return peaks.map((peak,i)=>{const height=Math.max(2,Math.min(1,Math.abs(peak))*180);return `M${i*step} ${(200-height)/2}h${Math.max(.4,step*.7)}v${height}h-${Math.max(.4,step*.7)}Z`;}).join("");
}
