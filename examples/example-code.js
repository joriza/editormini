// example-code.js — JavaScript sample
const http = require('http');
const url = require('url');

class Server {
  constructor(port) {
    this.port = port;
    this.routes = new Map();
  }

  get(path, handler) {
    this.routes.set(path, handler);
  }

  start() {
    const server = http.createServer((req, res) => {
      const parsedUrl = url.parse(req.url, true);
      const handler = this.routes.get(parsedUrl.pathname);
      
      if (handler) {
        handler(req, res);
      } else {
        res.writeHead(404, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: 'Not found' }));
      }
    });

    server.listen(this.port, () => {
      console.log(`Server running on port ${this.port}`);
    });
  }
}

const app = new Server(3000);
app.start();
