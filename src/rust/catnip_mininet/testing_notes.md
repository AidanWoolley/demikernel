# Testing Notes
  - `iptables -t raw -A PREROUTING -p tcp -j DROP` to prevent kernel interfering with catnip in each mininet host