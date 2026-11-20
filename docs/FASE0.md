# Fase 0 — resultado da descoberta

Ferramenta: `tools/probe` (Rust, só desenvolvimento). Rodar: `cargo run --manifest-path tools/probe/Cargo.toml`.
Data: 2026-11-19. Máquina: desktop, sem bateria interna.

## Tabela dispositivo → fonte de bateria

| Dispositivo | Identificação | Fonte que funciona | Resultado |
|---|---|---|---|
| Celular | Samsung SM-A315G, (nome dado pelo usuário ao aparelho) (04E8:6860) | **ADB** `dumpsys battery` (fonte confiável). WPD `WPD_DEVICE_POWER_LEVEL` (pid 4) só como reserva | ADB: **100%, carga USB, status cheio**. WPD no mesmo instante: **97** e "fonte = bateria". **O WPD está defasado e não serve como fonte principal.** |
| Receptor "USB Dongle" | 05AC:024F, interfaces: teclado, controle de mídia, mouse, página vendor 0xFFB5 (report 181) | HID padrão: **nenhuma** usage de bateria (página 0x06/0x20 ou 0x85) | Sem bateria padrão. Existe canal proprietário (0xFFB5), protocolo desconhecido. Mostrar `?`. |
| Receptor "Wireless-Receiver" | 249A:5C2F, interfaces: mouse, teclado, mídia, página vendor 0xFF01 (33 B in/out) | HID padrão: **nenhuma** | Sem bateria padrão. Existe canal proprietário (0xFF01), protocolo desconhecido. Mostrar `?`. |
| Teclado virtual G HUB | 046D:C232 | — | Dispositivo virtual de software. Excluir. |
| Bluetooth (fone QCY etc.) | Adaptador Intel OK; nenhum dispositivo BT conectado durante o teste | Propriedade PnP `DEVPKEY_Bluetooth_Battery` | **Não validado**: aparelho desligado. Validar na fase 1 quando ligado. |

## Conclusões que mudam o plano

1. **Celular:** a verificação cruzada (feita após a revisão do agente verificador) mostrou WPD=97 contra ADB=100 e WPD indicando "bateria" com o cabo ligado. O WPD é um instantâneo do momento da conexão, então **a fonte principal passa a ser o ADB**; o WPD fica como reserva marcada como "pode estar defasado". O ADB precisa de um `adb.exe` (não está no PATH; existe uma cópia em `C:\Program Files (x86)\Iriun Webcam\`).
2. **Mouse e teclado dos receptores:** nenhum expõe bateria pelo HID padrão. Ambos aparecem como `?`. Os canais proprietários (0xFF01 e 0xFFB5) só dariam bateria com engenharia reversa do protocolo, o que fica fora do MVP.
3. Os dois receptores são "combo" (mouse + teclado no mesmo USB). Cada receptor representa **um ícone** (agrupar pelo dispositivo USB pai), não um por interface.
4. Não consegui identificar marca e modelo reais do mouse e do teclado, apenas os nomes de produto dos receptores.

## Limitações da sondagem

- O probe só detecta bateria declarada no descritor HID (página 0x06 usage 0x20 e página 0x85). Não lê relatórios vendor.
- O percentual HID só seria lido se existisse; como não existiu, a leitura de relatório HID não foi exercitada.
- O Bluetooth ficou sem teste de ponta a ponta.

## Correções após a verificação independente

- Valor do WPD validado contra o ADB: divergente (97 x 100), conclusão do celular corrigida acima.
- Filtro de caminhos do probe passou a aceitar `vid&` (HID Bluetooth), além de `vid_`.
- Pendente para a fase 2: sondar os canais vendor 0xFF01 e 0xFFB5 dos receptores (hoje apenas detectados, não lidos).
- Ponto cego conhecido do probe: ignora *button caps* e output reports, e não checa `IsRange` das usages.
- Para o ADB foi iniciado e depois encerrado (`kill-server`) um servidor temporário usando o `adb.exe` do Iriun.

