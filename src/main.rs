fn main() {
    let cpu = ail::cpu_fam::info();
    let cpu_arch = ail::cpu_arch::detect_arch_and_version();
    println!("CPU: {cpu_arch} {cpu}");

    let os = ail::os::info();
    println!("OS: {}", os);
}
