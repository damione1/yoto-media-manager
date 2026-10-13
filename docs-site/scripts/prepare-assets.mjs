import { mkdir, copyFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const destination = new URL('../static/img/', import.meta.url);
await mkdir(destination, { recursive: true });
for (const name of ['app-icon.ico', 'portal-logo.png']) {
  await copyFile(new URL(`../../assets/branding/${name}`, import.meta.url),
    fileURLToPath(new URL(name, destination)));
}
