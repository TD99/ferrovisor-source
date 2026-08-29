.section .rodata.tiny_guest,"a",@progbits
.code64
.global tiny_guest_start
.global tiny_guest_end

tiny_guest_start:
    cli
    cld
    lea rsi, [rip + banner]
    call puts

shell:
    lea rsi, [rip + prompt]
    call puts
    call read_line

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_help]
    call string_equal
    test al, al
    jnz do_help

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_info]
    call string_equal
    test al, al
    jnz do_info

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_clear]
    call string_equal
    test al, al
    jnz do_clear

    lea rsi, [rip + input_buffer]
    lea rdi, [rip + cmd_halt]
    call string_equal
    test al, al
    jnz do_halt

    cmp byte ptr [rip + input_buffer], 0
    je shell
    lea rsi, [rip + unknown]
    call puts
    jmp shell

do_help:
    lea rsi, [rip + help_text]
    call puts
    jmp shell

do_info:
    xor eax, eax
    cpuid
    lea rsi, [rip + info_text]
    call puts
    jmp shell

do_clear:
    mov al, 0x0c
    call putchar
    jmp shell

do_halt:
    lea rsi, [rip + halt_text]
    call puts
    hlt
    jmp do_halt

read_line:
    lea rdi, [rip + input_buffer]
    xor ecx, ecx
.read_key:
    call getchar
    cmp al, 10
    je .done
    cmp al, 8
    je .backspace
    cmp ecx, 62
    jae .read_key
    stosb
    inc ecx
    call putchar
    jmp .read_key
.backspace:
    test ecx, ecx
    jz .read_key
    dec rdi
    dec ecx
    call putchar
    jmp .read_key
.done:
    mov byte ptr [rdi], 0
    call putchar
    ret

getchar:
    push rdx
    mov dx, 0x3f8
    in al, dx
    pop rdx
    ret

putchar:
    push rdx
    mov dx, 0x3f8
    out dx, al
    pop rdx
    ret

puts:
    push rax
    push rdx
    mov dx, 0x3f8
.next:
    lodsb
    test al, al
    jz .end
    out dx, al
    jmp .next
.end:
    pop rdx
    pop rax
    ret

string_equal:
.compare:
    mov al, byte ptr [rsi]
    cmp al, byte ptr [rdi]
    jne .different
    test al, al
    je .equal
    inc rsi
    inc rdi
    jmp .compare
.equal:
    mov al, 1
    ret
.different:
    xor eax, eax
    ret

banner:
    .asciz "\nTiny64 guest OS\nRunning inside AMD-V/SVM\nType 'help' for commands.\n\n"
prompt:
    .asciz "tiny> "
help_text:
    .asciz "help  - list commands\ninfo  - execute virtualized CPUID\nclear - clear display\nhalt  - stop the guest\n"
info_text:
    .asciz "64-bit guest active; CPUID VMEXIT handled by Ferrovisor.\n"
unknown:
    .asciz "unknown command\n"
halt_text:
    .asciz "guest halted\n"
cmd_help:  .asciz "help"
cmd_info:  .asciz "info"
cmd_clear: .asciz "clear"
cmd_halt:  .asciz "halt"

.balign 16
input_buffer:
    .zero 64
tiny_guest_end:
