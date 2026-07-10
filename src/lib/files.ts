const SUPPORTED_FILE = /\.(?:png|jpe?g|webp|pdf)$/i;

export const filterSupported = (paths: string[]) =>
  paths.filter((path) => SUPPORTED_FILE.test(path));
