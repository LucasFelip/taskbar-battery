# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/); versionamento [SemVer](https://semver.org/lang/pt-BR/).

## [0.2.0] - 2026-11-19

### Adicionado
- **Um ícone por dispositivo, em qualquer conexão.** Dispositivos da tabela `src/known.rs` (hoje o teclado GMK67, VID/PID 05AC:024F) aparecem como um único ícone, quer estejam por cabo, receptor 2,4 GHz ou Bluetooth. O tooltip mostra as conexões ativas, por exemplo `[2,4 GHz, cabo]`.
- **Último valor de bateria.** Quando o dispositivo está num modo que não informa bateria, o ícone mostra, esmaecido, o último valor conhecido (o do Bluetooth, guardado em `last-known.ini` ou, se não houver, o que o Windows guardou para o dispositivo pareado), com a idade no tooltip ("há 1 h", "há 3 dias"). Valores antigos não disparam o alerta de bateria baixa.

## [0.1.0] - 2026-11-19

Primeira versão.

### Adicionado
- Um ícone de bandeja por dispositivo, com três estilos (bateria, número, tipo + barra de nível) e cor vermelha abaixo de 20%.
- Bateria de dispositivos **Bluetooth** conectados (propriedade PnP do Windows, no nó do dispositivo ou no nó Hands-Free) e tipo do dispositivo pela Class of Device.
- Bateria de **celular Android por USB** via `adb shell dumpsys battery` (servidor ADB iniciado só durante a leitura).
- Leitura de bateria **HID padrão** (Battery Strength) de dispositivos USB.
- Leitura passiva do relatório proprietário de bateria do receptor 2.4 GHz do **mouse BK-R3X** (249A:5C2F); o último valor é guardado em disco.
- Receptores e dispositivos USB sem bateria legível aparecem com `?`.
- Menu de contexto: lista de dispositivos com caixa de seleção (ocultar/mostrar), atualizar, estilo do ícone, alerta de bateria baixa (15%), fixar ícones na barra, iniciar com o Windows, sondagem USB e cópia do diagnóstico.
- Atualização por timer (10 min), por conectar/desconectar dispositivos e ao acordar do sleep.
- Último valor esmaecido quando uma leitura falha (expira após 3 ciclos).
- `--once`, `--fake`, `--probe` e `--version` na linha de comando; log de diagnóstico anonimizado.
- Instalação sem administrador (`scripts/install.ps1`) e desinstalador (`scripts/uninstall.ps1`).
- CI (fmt, clippy, testes, build, smoke test) e pipeline de release por tag.

### Limitações conhecidas
- Bluetooth LE via GATT (WinRT) não implementado.
- Receptores 2.4 GHz proprietários (por exemplo o do teclado GMK67) não informam bateria; ela só aparece pelo Bluetooth.
- Executável sem ícone, versão embutida e assinatura digital.

[0.2.0]: https://github.com/LucasFelip/taskbar-battery/releases/tag/v0.2.0
[0.1.0]: https://github.com/LucasFelip/taskbar-battery/releases/tag/v0.1.0
