import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import viteTsconfigPaths from 'vite-tsconfig-paths'
import mkcert from 'vite-plugin-mkcert'

export default defineConfig({
    base: process.env.BASE_URL ?? './',
    plugins: [react(), viteTsconfigPaths(), mkcert()],
    server: {
        open: process.env.BROWSER='chrome',
        // Bind all interfaces so both 127.0.0.1 (IPv4) and ::1 (IPv6) work;
        // some browsers resolve `localhost` to IPv4 only.
        host: true,
        port: 3000,
        https: true,
    },
    build: {
        sourcemap: true
    },
    resolve: {
    }
});
