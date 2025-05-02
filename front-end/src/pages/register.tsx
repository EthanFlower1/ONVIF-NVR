import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Logo } from '../components/logo'
import { Button } from '../components/button'
import { Checkbox, CheckboxField } from '../components/checkbox'
import { Field, Label } from '../components/fieldset'
import { Heading } from '../components/heading'
import { Input } from '../components/input'
import { Select } from '../components/select'
import { Strong, Text, TextLink } from '../components/text'
import { useAuth } from '../contexts/AuthContext'

export default function Register() {
  const [username, setUsername] = useState('')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [country, setCountry] = useState('United States')
  const [serverUrl, setServerUrl] = useState('')
  const [marketing, setMarketing] = useState(false)
  const [error, setError] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const [success, setSuccess] = useState(false)
  
  const { register, setApiBaseUrl } = useAuth()
  const navigate = useNavigate()
  
  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    
    if (!username || !email || !password || !serverUrl) {
      setError('All fields are required')
      return
    }
    
    // Store the server URL for API requests
    setApiBaseUrl(serverUrl)
    
    setIsLoading(true)
    setError('')
    
    try {
      await register(username, email, password)
      setSuccess(true)
      // Redirect to login after 1.5 seconds
      setTimeout(() => {
        navigate('/login', { 
          state: { 
            message: 'Registration successful! Please login with your new account.' 
          } 
        })
      }, 1500)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Registration failed')
    } finally {
      setIsLoading(false)
    }
  }
  
  return (
    <form onSubmit={handleSubmit} className="grid w-full max-w-sm grid-cols-1 gap-8">
      <Logo className="h-6 text-zinc-950 dark:text-white forced-colors:text-[CanvasText]" />
      <Heading>Create your account</Heading>
      
      {error && (
        <div className="bg-red-50 border border-red-200 text-red-800 px-4 py-3 rounded">
          {error}
        </div>
      )}
      
      {success && (
        <div className="bg-green-50 border border-green-200 text-green-800 px-4 py-3 rounded">
          Registration successful! Redirecting to login...
        </div>
      )}
      
      <Field>
        <Label>Server URL</Label>
        <Input 
          type="text" 
          name="serverUrl" 
          value={serverUrl} 
          onChange={(e) => setServerUrl(e.target.value)} 
          placeholder="http://localhost:8080"
          required 
        />
      </Field>
      
      <Field>
        <Label>Username</Label>
        <Input 
          type="text" 
          name="username" 
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          required
        />
      </Field>
      <Field>
        <Label>Email</Label>
        <Input 
          type="email" 
          name="email" 
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          required
        />
      </Field>
      <Field>
        <Label>Password</Label>
        <Input 
          type="password" 
          name="password" 
          autoComplete="new-password" 
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
        />
      </Field>
      <Field>
        <Label>Country</Label>
        <Select 
          name="country"
          value={country}
          onChange={(e) => setCountry(e.target.value)}
        >
          <option>Canada</option>
          <option>Mexico</option>
          <option>United States</option>
        </Select>
      </Field>
      <CheckboxField>
        <Checkbox 
          name="marketing" 
          checked={marketing}
          onChange={(e) => setMarketing(e.target.checked)}
        />
        <Label>Get emails about product updates and news.</Label>
      </CheckboxField>
      <Button 
        type="submit" 
        className="w-full"
        disabled={isLoading || success}
      >
        {isLoading ? 'Creating account...' : 'Create account'}
      </Button>
      <Text>
        Already have an account?{' '}
        <TextLink href="/login">
          <Strong>Sign in</Strong>
        </TextLink>
      </Text>
    </form>
  )
}