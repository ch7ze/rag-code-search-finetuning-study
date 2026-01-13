"""
GPU Performance Monitoring Utility
Tracks GPU utilization, memory usage, and performance metrics during evaluation
"""

import time
import torch
from typing import Dict, List
from contextlib import contextmanager


class GPUMonitor:
    """Monitor GPU performance metrics"""

    def __init__(self):
        self.enabled = torch.cuda.is_available()
        self.metrics = []
        self.start_time = None

        if self.enabled:
            print(f"GPU Monitor initialized: {torch.cuda.get_device_name(0)}")
            print(f"Total GPU Memory: {torch.cuda.get_device_properties(0).total_memory / 1e9:.2f} GB\n")
        else:
            print("GPU Monitor: CUDA not available (CPU mode)\n")

    def capture_snapshot(self, label: str = ""):
        """Capture current GPU state"""
        if not self.enabled:
            return

        snapshot = {
            'timestamp': time.time(),
            'label': label,
            'memory_allocated_mb': torch.cuda.memory_allocated() / 1e6,
            'memory_reserved_mb': torch.cuda.memory_reserved() / 1e6,
            'memory_cached_mb': torch.cuda.memory_reserved() / 1e6,
        }

        self.metrics.append(snapshot)
        return snapshot

    @contextmanager
    def track_operation(self, operation_name: str):
        """Context manager to track GPU usage during an operation"""
        if not self.enabled:
            yield
            return

        # Start tracking
        torch.cuda.synchronize()
        start_memory = torch.cuda.memory_allocated()
        start_time = time.time()

        try:
            yield
        finally:
            # End tracking
            torch.cuda.synchronize()
            end_time = time.time()
            end_memory = torch.cuda.memory_allocated()

            metric = {
                'operation': operation_name,
                'duration_ms': (end_time - start_time) * 1000,
                'memory_change_mb': (end_memory - start_memory) / 1e6,
                'peak_memory_mb': torch.cuda.max_memory_allocated() / 1e6
            }

            self.metrics.append(metric)

            # Print real-time feedback
            print(f"  [{operation_name}] {metric['duration_ms']:.1f}ms, "
                  f"Mem: {metric['memory_change_mb']:+.1f}MB")

    def print_summary(self):
        """Print summary of GPU usage"""
        if not self.enabled:
            print("GPU Monitor: No metrics (CPU mode)")
            return

        print("\n" + "="*70)
        print("GPU PERFORMANCE SUMMARY")
        print("="*70)

        # Current state
        mem_allocated = torch.cuda.memory_allocated() / 1e9
        mem_reserved = torch.cuda.memory_reserved() / 1e9
        max_mem = torch.cuda.max_memory_allocated() / 1e9

        print(f"\nMemory Usage:")
        print(f"  Current Allocated: {mem_allocated:.2f} GB")
        print(f"  Reserved (Cached): {mem_reserved:.2f} GB")
        print(f"  Peak Allocated:    {max_mem:.2f} GB")

        # Operation timings
        if self.metrics:
            operations = [m for m in self.metrics if 'operation' in m]
            if operations:
                print(f"\nOperation Timings:")
                for op in operations:
                    print(f"  {op['operation']:30s} {op['duration_ms']:8.1f}ms  "
                          f"(Mem: {op['memory_change_mb']:+.1f}MB)")

        print("="*70 + "\n")

    def reset(self):
        """Reset all metrics"""
        self.metrics = []
        if self.enabled:
            torch.cuda.reset_peak_memory_stats()


def print_gpu_info():
    """Print detailed GPU information"""
    if not torch.cuda.is_available():
        print("CUDA not available - running on CPU")
        return

    print("="*70)
    print("GPU INFORMATION")
    print("="*70)

    for i in range(torch.cuda.device_count()):
        props = torch.cuda.get_device_properties(i)
        print(f"\nGPU {i}: {props.name}")
        print(f"  Compute Capability: {props.major}.{props.minor}")
        print(f"  Total Memory: {props.total_memory / 1e9:.2f} GB")
        print(f"  Multi-Processors: {props.multi_processor_count}")

    print("\nCurrent Status:")
    print(f"  Active Device: cuda:{torch.cuda.current_device()}")
    print(f"  Memory Allocated: {torch.cuda.memory_allocated() / 1e9:.3f} GB")
    print(f"  Memory Reserved: {torch.cuda.memory_reserved() / 1e9:.3f} GB")
    print("="*70 + "\n")


if __name__ == "__main__":
    print_gpu_info()

    # Example usage
    if torch.cuda.is_available():
        monitor = GPUMonitor()

        with monitor.track_operation("Test Operation"):
            # Simulate GPU work
            x = torch.randn(1000, 1000, device='cuda')
            y = torch.matmul(x, x)
            time.sleep(0.1)

        monitor.print_summary()
