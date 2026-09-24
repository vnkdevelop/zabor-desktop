{
  "targets": [
    {
      "target_name": "zabor_low_latency_audio",
      "include_dirs": [
        "<!@(node -p \"require('node-addon-api').include\")"
      ],
      "dependencies": [
        "<!(node -p \"require('node-addon-api').gyp\")"
      ],
      "defines": [
        "NAPI_DISABLE_CPP_EXCEPTIONS"
      ],
      "conditions": [
        ["OS==\"win\"", {
          "sources": [
            "src/probe.cc"
          ],
          "defines": [
            "_WIN32_WINNT=0x0A00",
            "NTDDI_VERSION=0x0A00000A"
          ],
          "libraries": [
            "-lMmdevapi.lib",
            "-lOle32.lib"
          ]
        }]
      ]
    }
  ]
}
