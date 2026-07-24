# Parity (Plan 036 gap-1 fix): uniq dedupes adjacent identical lines.
# Uses sort|uniq (the previously-broken pipe-stage path).
> echo "banana" > p60uniq.txt
> echo "apple" >> p60uniq.txt
> echo "banana" >> p60uniq.txt
> echo "apple" >> p60uniq.txt
> cat p60uniq.txt | sort | uniq
