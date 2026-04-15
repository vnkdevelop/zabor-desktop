const { Client } = require('ssh2');
const conn = new Client();
conn.on('ready', () => {
    conn.exec('grep -l "listen 8080" /etc/nginx/sites-enabled/* /etc/nginx/conf.d/* /etc/nginx/nginx.conf 2>/dev/null', (err, stream) => {
        if (err) throw err;
        stream.on('close', () => conn.end()).on('data', d => console.log(d.toString().trim()));
    });
}).connect({ host: '150.241.64.108', port: 22, username: 'root', password: 'mvtxbJo45sc8' });
