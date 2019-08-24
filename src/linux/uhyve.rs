//! This file contains the entry point to the Hypervisor. The Uhyve utilizes KVM to
//! create a Virtual Machine and load the kernel.

use error::*;
use kvm_bindings::*;
use kvm_ioctls::VmFd;
use libc;
use linux::vcpu::*;
use linux::{MemoryRegion, KVM};
use std;
use std::convert::TryInto;
use std::intrinsics::volatile_load;
use std::ptr;
use vm::{KernelHeaderV0, VirtualCPU, Vm, VmParameter};

const KVM_32BIT_MAX_MEM_SIZE: usize = 1 << 32;
const KVM_32BIT_GAP_SIZE: usize = 768 << 20;
const KVM_32BIT_GAP_START: usize = KVM_32BIT_MAX_MEM_SIZE - KVM_32BIT_GAP_SIZE;

pub struct Uhyve {
	vm: VmFd,
	entry_point: u64,
	mem: MmapMemory,
	num_cpus: u32,
	path: String,
	kernel_header: *const KernelHeaderV0,
	verbose: bool,
}

impl Uhyve {
	pub fn new(kernel_path: String, specs: &VmParameter) -> Result<Uhyve> {
		let vm = KVM.create_vm().or_else(to_error)?;

		let mut cap: kvm_enable_cap = Default::default();
		cap.cap = KVM_CAP_SET_TSS_ADDR;
		if vm.enable_cap(&cap).is_ok() {
			debug!("Setting TSS address");
			vm.set_tss_address(0xfffbd000).or_else(to_error)?;
		}

		let mem = MmapMemory::new(0, specs.mem_size, 0, specs.hugepage, specs.mergeable);

		let sz = if specs.mem_size < KVM_32BIT_GAP_START {
			specs.mem_size
		} else {
			KVM_32BIT_GAP_START
		};

		let kvm_mem = kvm_userspace_memory_region {
			slot: 0,
			flags: mem.flags(),
			memory_size: sz as u64,
			guest_phys_addr: mem.guest_address() as u64,
			userspace_addr: mem.host_address() as u64,
		};

		unsafe { vm.set_user_memory_region(kvm_mem) }.or_else(to_error)?;

		if specs.mem_size > KVM_32BIT_GAP_START + KVM_32BIT_GAP_SIZE {
			let kvm_mem = kvm_userspace_memory_region {
				slot: 1,
				flags: mem.flags(),
				memory_size: (specs.mem_size - KVM_32BIT_GAP_START - KVM_32BIT_GAP_SIZE) as u64,
				guest_phys_addr: (mem.guest_address() + KVM_32BIT_GAP_START + KVM_32BIT_GAP_SIZE)
					as u64,
				userspace_addr: (mem.host_address() + KVM_32BIT_GAP_START + KVM_32BIT_GAP_SIZE)
					as u64,
			};

			unsafe { vm.set_user_memory_region(kvm_mem) }.or_else(to_error)?;
		}

		let mut hyve = Uhyve {
			vm: vm,
			entry_point: 0,
			mem: mem,
			num_cpus: specs.num_cpus,
			path: kernel_path,
			kernel_header: ptr::null(),
			verbose: specs.verbose,
		};

		hyve.init()?;

		Ok(hyve)
	}

	fn init(&mut self) -> Result<()> {
		self.init_guest_mem();

		debug!("Initialize interrupt controller");

		// create basic interrupt controller
		self.vm.create_irq_chip().or_else(to_error)?;
		let pit_config = kvm_pit_config::default();
		self.vm.create_pit2(pit_config).or_else(to_error)?;

		// currently, we support only system, which provides the
		// cpu feature TSC_DEADLINE
		let mut cap: kvm_enable_cap = Default::default();
		cap.cap = KVM_CAP_TSC_DEADLINE_TIMER;
		if self.vm.enable_cap(&cap).is_ok() {
			panic!("Processor feature \"tsc deadline\" isn't supported!")
		}

		Ok(())
	}
}

impl Vm for Uhyve {
	fn verbose(&self) -> bool {
		self.verbose
	}

	fn set_entry_point(&mut self, entry: u64) {
		self.entry_point = entry;
	}

	fn get_entry_point(&self) -> u64 {
		self.entry_point
	}

	fn num_cpus(&self) -> u32 {
		self.num_cpus
	}

	fn guest_mem(&self) -> (*mut u8, usize) {
		(self.mem.host_address() as *mut u8, self.mem.memory_size())
	}

	fn kernel_path(&self) -> &str {
		&self.path
	}

	fn create_cpu(&self, id: u32) -> Result<Box<dyn VirtualCPU>> {
		let vm_start = self.mem.host_address() as usize;
		Ok(Box::new(UhyveCPU::new(
			id,
			self.path.clone(),
			self.vm
				.create_vcpu(id.try_into().unwrap())
				.or_else(to_error)?,
			vm_start,
		)))
	}

	fn set_kernel_header(&mut self, header: *const KernelHeaderV0) {
		self.kernel_header = header;
	}

	fn cpu_online(&self) -> u32 {
		if self.kernel_header.is_null() {
			0
		} else {
			unsafe { volatile_load(&(*self.kernel_header).cpu_online) }
		}
	}
}

impl Drop for Uhyve {
	fn drop(&mut self) {
		debug!("Drop virtual machine");
	}
}

unsafe impl Send for Uhyve {}
unsafe impl Sync for Uhyve {}

#[derive(Debug)]
struct MmapMemory {
	flags: u32,
	memory_size: usize,
	guest_address: usize,
	host_address: usize,
}

impl MmapMemory {
	pub fn new(
		flags: u32,
		memory_size: usize,
		guest_address: u64,
		huge_pages: bool,
		mergeable: bool,
	) -> MmapMemory {
		let host_address = unsafe {
			libc::mmap(
				std::ptr::null_mut(),
				memory_size,
				libc::PROT_READ | libc::PROT_WRITE,
				libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
				-1,
				0,
			)
		};

		if host_address == libc::MAP_FAILED {
			panic!("mmap failed with: {}", unsafe { *libc::__errno_location() });
		}

		if mergeable {
			debug!("Enable kernel feature to merge same pages");
			let ret = unsafe { libc::madvise(host_address, memory_size, libc::MADV_MERGEABLE) };

			if ret < 0 {
				panic!("madvise failed with: {}", unsafe {
					*libc::__errno_location()
				});
			}
		}

		if huge_pages {
			debug!("Uhyve uses huge pages");
			let ret = unsafe { libc::madvise(host_address, memory_size, libc::MADV_HUGEPAGE) };

			if ret < 0 {
				panic!("madvise failed with: {}", unsafe {
					*libc::__errno_location()
				});
			}
		}

		MmapMemory {
			flags: flags,
			memory_size: memory_size,
			guest_address: guest_address as usize,
			host_address: host_address as usize,
		}
	}

	#[allow(dead_code)]
	fn as_slice_mut(&mut self) -> &mut [u8] {
		unsafe { std::slice::from_raw_parts_mut(self.host_address as *mut u8, self.memory_size) }
	}
}

impl MemoryRegion for MmapMemory {
	fn flags(&self) -> u32 {
		self.flags
	}

	fn memory_size(&self) -> usize {
		self.memory_size
	}

	fn guest_address(&self) -> usize {
		self.guest_address
	}

	fn host_address(&self) -> usize {
		self.host_address
	}
}

impl Drop for MmapMemory {
	fn drop(&mut self) {
		if self.memory_size() > 0 {
			let result = unsafe {
				libc::munmap(self.host_address() as *mut libc::c_void, self.memory_size())
			};
			if result != 0 {
				panic!("munmap failed with: {}", unsafe {
					*libc::__errno_location()
				});
			}
		}
	}
}
