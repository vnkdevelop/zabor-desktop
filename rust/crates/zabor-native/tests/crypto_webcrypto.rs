use std::io::Write;
use std::process::{Command, Stdio};
use zabor_native::chat_crypto::{decrypt, ChatIdentity};

#[test]
fn p256_and_aes_gcm_interoperate_with_the_existing_webcrypto_wire_format() {
    let identity = ChatIdentity::generate();
    let script = r#"
const { webcrypto } = require('node:crypto');
let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', value => input += value);
process.stdin.on('end', async () => {
  const remote = JSON.parse(input);
  const nativeKey = await webcrypto.subtle.importKey('jwk', remote, {name:'ECDH',namedCurve:'P-256'}, false, []);
  const pair = await webcrypto.subtle.generateKey({name:'ECDH',namedCurve:'P-256'}, true, ['deriveKey']);
  const key = await webcrypto.subtle.deriveKey({name:'ECDH',public:nativeKey}, pair.privateKey, {name:'AES-GCM',length:256}, false, ['encrypt']);
  const iv = webcrypto.getRandomValues(new Uint8Array(12));
  const data = await webcrypto.subtle.encrypt({name:'AES-GCM',iv}, key, new TextEncoder().encode('native ↔ webcrypto'));
  process.stdout.write(JSON.stringify({publicKey:await webcrypto.subtle.exportKey('jwk',pair.publicKey),packet:JSON.stringify({iv:Array.from(iv),data:Array.from(new Uint8Array(data))})}));
});
"#;
    let mut process = Command::new("node")
        .args(["-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(identity.public_key().unwrap().to_string().as_bytes())
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success(), "WebCrypto fixture failed");
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let key = identity.derive(&response["publicKey"]).unwrap();
    assert_eq!(
        decrypt(&key, response["packet"].as_str().unwrap())
            .unwrap()
            .as_str(),
        "native ↔ webcrypto"
    );
}
