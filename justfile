jtag:
  system_profiler SPUSBDataType | grep -A 11 "USB JTAG"

device:
  ls /dev/tty.usbmodem*

monitor:
  espflash monitor