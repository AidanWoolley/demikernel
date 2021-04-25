# Testing Notes
  - `sudo mn --custom=catnip_test_topo.py --topo=catniptopo -x`
  - `iptables -t raw -A PREROUTING -p tcp -j DROP` to prevent kernel interfering with catnip in each mininet host