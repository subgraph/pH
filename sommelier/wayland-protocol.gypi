# Caution!: GYP to GN migration is happening. If you update this file, please
# update vm_tools/sommelier/wayland-protocol.gni too accordingly.
{
  'variables': {
    'wayland_dir': '<(SHARED_INTERMEDIATE_DIR)/<(wayland_out_dir)',
    'wayland_in_dir%': '.',
  },
  'rules': [
    {
      'rule_name': 'genwayland',
      'extension': 'xml',
      'outputs': [
        '<(wayland_dir)/<(RULE_INPUT_ROOT)-protocol.c',
        '<(wayland_dir)/<(RULE_INPUT_ROOT)-client-protocol.h',
        '<(wayland_dir)/<(RULE_INPUT_ROOT)-server-protocol.h',
      ],
      'action': [
        'sh',
        '-c',
        'wayland-scanner code < <(wayland_in_dir)/<(RULE_INPUT_NAME) > <(wayland_dir)/<(RULE_INPUT_ROOT)-protocol.c; wayland-scanner client-header < <(wayland_in_dir)/<(RULE_INPUT_NAME) > <(wayland_dir)/<(RULE_INPUT_ROOT)-client-protocol.h; wayland-scanner server-header < <(wayland_in_dir)/<(RULE_INPUT_NAME) > <(wayland_dir)/<(RULE_INPUT_ROOT)-server-protocol.h',
      ],
      'message': 'Generating Wayland C code from <(RULE_INPUT_PATH)',
      'process_outputs_as_sources': 1,
    },
  ],
  # This target exports a hard dependency because it generates header
  # files.
  'hard_dependency': 1,
  'direct_dependent_settings': {
    'include_dirs': [
      '<(wayland_dir)',
    ],
  },
}
