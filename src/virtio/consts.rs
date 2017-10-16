
// Maximum number of logical devices on a PCI bus

pub const PCI_MAX_DEVICES: usize = 32;

// IO Port addresses for PCI configuration access

pub const PCI_CONFIG_ADDRESS: u16 = 0xcf8;
pub const PCI_CONFIG_DATA:    u16 = 0xcfc;

// Vendor specific PCI capabilities

pub const PCI_CAP_ID_VENDOR: u8 = 0x09;

pub const PCI_CONFIG_SPACE_SIZE: usize = 256;
pub const PCI_CAP_BASE_OFFSET: usize = 0x40;

pub const PCI_VENDOR_ID: usize = 0x00;
pub const PCI_DEVICE_ID: usize = 0x02;
pub const PCI_COMMAND: usize = 0x04;
pub const PCI_COMMAND_IO: u16 = 0x01;
pub const PCI_COMMAND_MEMORY: u16 = 0x02;
pub const PCI_COMMAND_INTX_DISABLE: u16 = 0x400;
pub const PCI_STATUS: usize = 0x06;
pub const PCI_STATUS_CAP_LIST: u16 = 0x10;
pub const PCI_CLASS_REVISION: usize = 0x08;
pub const PCI_CLASS_DEVICE: usize = 0x0a;
pub const PCI_CACHE_LINE_SIZE: usize = 0x0c;
pub const PCI_LATENCY_TIMER: usize = 0x0d;

pub const _PCI_SUBSYSTEM_VENDOR_ID: usize = 0x2c;
pub const PCI_SUBSYSTEM_ID: usize = 0x2e;
pub const PCI_CAPABILITY_LIST: usize = 0x34;
pub const PCI_INTERRUPT_LINE: usize = 0x3C;
pub const PCI_INTERRUPT_PIN: usize = 0x3D;

// Virtio PCI capability types

pub const VIRTIO_PCI_CAP_COMMON_CFG : u8 = 1;
pub const VIRTIO_PCI_CAP_NOTIFY_CFG : u8 = 2;
pub const VIRTIO_PCI_CAP_ISR_CFG    : u8 = 3;
pub const VIRTIO_PCI_CAP_DEVICE_CFG : u8 = 4;

// Indicates that no MSIX vector is configured

pub const VIRTIO_NO_MSI_VECTOR: u16 = 0xFFFF;

// Bar number 0 is used for Virtio MMIO area

pub const VIRTIO_MMIO_BAR: usize = 0;

// Virtio MMIO area is one page

pub const VIRTIO_MMIO_AREA_SIZE: usize = 4096;

// Offsets and sizes for each structure in MMIO area

pub const VIRTIO_MMIO_OFFSET_COMMON_CFG : usize = 0;     // Common configuration offset
pub const VIRTIO_MMIO_OFFSET_ISR        : usize = 56;    // ISR register offset
pub const VIRTIO_MMIO_OFFSET_NOTIFY     : usize = 0x400; // Notify area offset
pub const VIRTIO_MMIO_OFFSET_DEV_CFG    : usize = 0x800; // Device specific configuration offset

pub const VIRTIO_MMIO_COMMON_CFG_SIZE: usize = 56;       // Common configuration size
pub const VIRTIO_MMIO_NOTIFY_SIZE    : usize = 0x400;    // Notify area size
pub const VIRTIO_MMIO_ISR_SIZE       : usize = 4;        // ISR register size

// Common configuration header offsets

pub const VIRTIO_PCI_COMMON_DFSELECT      : usize = 0;
pub const VIRTIO_PCI_COMMON_DF            : usize = 4;
pub const VIRTIO_PCI_COMMON_GFSELECT      : usize = 8;
pub const VIRTIO_PCI_COMMON_GF            : usize = 12;
pub const VIRTIO_PCI_COMMON_MSIX          : usize = 16;
pub const VIRTIO_PCI_COMMON_NUMQ          : usize = 18;
pub const VIRTIO_PCI_COMMON_STATUS        : usize = 20;
pub const VIRTIO_PCI_COMMON_CFGGENERATION : usize = 21;
pub const VIRTIO_PCI_COMMON_Q_SELECT      : usize = 22;
pub const VIRTIO_PCI_COMMON_Q_SIZE        : usize = 24;
pub const VIRTIO_PCI_COMMON_Q_MSIX        : usize = 26;
pub const VIRTIO_PCI_COMMON_Q_ENABLE      : usize = 28;
pub const VIRTIO_PCI_COMMON_Q_NOFF        : usize = 30;
pub const VIRTIO_PCI_COMMON_Q_DESCLO      : usize = 32;
pub const VIRTIO_PCI_COMMON_Q_DESCHI      : usize = 36;
pub const VIRTIO_PCI_COMMON_Q_AVAILLO     : usize = 40;
pub const VIRTIO_PCI_COMMON_Q_AVAILHI     : usize = 44;
pub const VIRTIO_PCI_COMMON_Q_USEDLO      : usize = 48;
pub const VIRTIO_PCI_COMMON_Q_USEDHI      : usize = 52;

// Common configuration status bits

pub const _VIRTIO_CONFIG_S_ACKNOWLEDGE : u8 = 1;
pub const _VIRTIO_CONFIG_S_DRIVER      : u8 = 2;
pub const VIRTIO_CONFIG_S_DRIVER_OK   : u8 = 4;
pub const VIRTIO_CONFIG_S_FEATURES_OK : u8 = 8;
pub const VIRTIO_CONFIG_S_NEEDS_RESET : u8 = 0x40;
pub const _VIRTIO_CONFIG_S_FAILED      : u8 = 0x80;

pub const _VRING_USED_F_NO_NOTIFY: u16 = 1;
pub const _VRING_AVAIL_F_NO_INTERRUPT: u16 = 1;
pub const _VIRTIO_F_INDIRECT_DESC: u64 = (1 << 28);
pub const VIRTIO_F_EVENT_IDX: u64 = (1 << 29);
pub const VIRTIO_F_VERSION_1: u64 = (1 << 32);

pub const VRING_DESC_F_NEXT: u16     = 1;
pub const VRING_DESC_F_WRITE: u16    = 2;
pub const VRING_DESC_F_INDIRECT: u16 = 4;

pub const DEFAULT_QUEUE_SIZE: u16 = 128;
pub const MAX_QUEUE_SIZE: u16 = 1024;

// PCI Vendor id for Virtio devices

pub const PCI_VENDOR_ID_REDHAT: u16 = 0x1af4;

// Base PCI device id for Virtio devices

pub const PCI_VIRTIO_DEVICE_ID_BASE: u16 = 0x1040;

pub const PCI_VENDOR_ID_INTEL: u16 = 0x8086;
pub const PCI_CLASS_BRIDGE_HOST: u16 = 0x0600;
