interface Size {
  width: number;
  height: number;
}

export function scaleBbox(
  [x1, y1, x2, y2]: [number, number, number, number],
  page: Size,
  rendered: Size,
) {
  const xScale = rendered.width / page.width;
  const yScale = rendered.height / page.height;
  return {
    left: x1 * xScale,
    top: y1 * yScale,
    width: (x2 - x1) * xScale,
    height: (y2 - y1) * yScale,
  };
}
