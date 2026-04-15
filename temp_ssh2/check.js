const { Client } = require('ssh2');
const conn = new Client();
conn.on('ready', () => {
    conn.exec('curl -kv https://vnkboltik.ru:8443/zabor_v3', (err, stream) => {
        if (err) throw err;
        stream.on('close', () => conn.end()).on('data', d => process.stdout.write(d));
        stream.stderr.on('data', d => process.stderr.write(d));
    });
}).connect({ host: '150.241.64.108', port: 22, username: 'root', password: 'mvtxbJo45sc8' });
