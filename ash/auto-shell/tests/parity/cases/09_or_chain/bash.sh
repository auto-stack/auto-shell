#!/bin/bash
cat /no/such/file/here >/dev/null 2>&1 || echo fallback
